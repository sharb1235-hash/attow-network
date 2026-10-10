use crate::{
    ProposalContext, SyncReport, TransferObject, TransportError,
    endpoint::{Inner, Lease},
};
use chronos_codec::ObjectId;
use chronos_protocol::{
    Ack, Chunk, ErrorMessage, Hello, Message, PROTOCOL_VERSION, PromotionReply, ProposalEnvelope,
    ProtocolLimits, decode_frame, decode_header, encode_frame,
};
use iroh::endpoint::{Connection, RecvStream, SendStream};
use std::{collections::BTreeSet, sync::Arc};

/// Close a connection on every exit, including errors and task cancellation.
/// Otherwise the responder can retain a slot until its operation deadline.
struct CloseOnDrop(Connection);
impl Drop for CloseOnDrop {
    fn drop(&mut self) {
        self.0.close(0u32.into(), b"operation finished");
    }
}

fn hello(inner: &Inner) -> Message {
    Message::Hello(Hello {
        version: PROTOCOL_VERSION,
        tenant: inner.scope.tenant,
        domain: inner.scope.domain,
    })
}
fn check_hello(inner: &Inner, message: Message) -> Result<(), TransportError> {
    if message == hello(inner) {
        Ok(())
    } else {
        Err(TransportError::Unauthorized)
    }
}
fn session_proof(inner: &Inner, connection: &Connection) -> Result<[u8; 32], TransportError> {
    let mut context = [0u8; 64];
    context[..32].copy_from_slice(&inner.scope.tenant.0);
    context[32..].copy_from_slice(&inner.scope.domain.0);
    let mut proof = [0u8; 32];
    connection
        .export_keying_material(&mut proof, b"attow/1/handshake-confirmed", &context)
        .map_err(|_| TransportError::Protocol)?;
    Ok(proof)
}
async fn initiate(
    inner: &Inner,
    connection: &Connection,
    send: &mut SendStream,
    recv: &mut RecvStream,
) -> Result<(), TransportError> {
    // Initiator bytes make the stream visible. The fixed-width proof then binds
    // every operation to this completed handshake, not a replayable TLS ticket.
    write(send, &hello(inner), &inner.limits.protocol).await?;
    check_hello(inner, read(recv, &inner.limits.protocol).await?)?;
    send.write_all(&session_proof(inner, connection)?)
        .await
        .map_err(|_| TransportError::Unavailable)
}
async fn write(
    send: &mut SendStream,
    message: &Message,
    limits: &ProtocolLimits,
) -> Result<(), TransportError> {
    let bytes = encode_frame(message)?;
    decode_frame(&bytes, limits)?;
    send.write_all(&bytes)
        .await
        .map_err(|_| TransportError::Unavailable)
}
async fn read(recv: &mut RecvStream, limits: &ProtocolLimits) -> Result<Message, TransportError> {
    let mut header = [0u8; 5];
    recv.read_exact(&mut header)
        .await
        .map_err(|_| TransportError::Protocol)?;
    let parsed = decode_header(&header, limits)?;
    // Reject the size/type before allocating the remaining body.
    let mut bytes = vec![0u8; parsed.payload_bytes + 4];
    bytes[..5].copy_from_slice(&header);
    recv.read_exact(&mut bytes[5..])
        .await
        .map_err(|_| TransportError::Protocol)?;
    let message = decode_frame(&bytes, limits)?;
    match message {
        Message::ProtocolError(error) => Err(match error.code {
            1 => TransportError::Unauthorized,
            2 => TransportError::Limit,
            3 => TransportError::Storage,
            4 => TransportError::Busy,
            5 => TransportError::Application,
            _ => TransportError::Protocol,
        }),
        _ => Ok(message),
    }
}
pub(crate) async fn serve(
    inner: Arc<Inner>,
    connection: Connection,
    peer: [u8; 32],
) -> Result<(), TransportError> {
    let _connection_lifetime = CloseOnDrop(connection.clone());
    let (mut send, mut recv) = connection
        .accept_bi()
        .await
        .map_err(|_| TransportError::Unavailable)?;
    let result = async {
        let control = inner.lease(peer, false)?;
        let hello_limits = ProtocolLimits {
            control_bytes: 256,
            ..inner.limits.protocol
        };
        check_hello(&inner, read(&mut recv, &hello_limits).await?)?;
        write(&mut send, &hello(&inner), &inner.limits.protocol).await?;
        let mut proof = [0u8; 32];
        recv.read_exact(&mut proof)
            .await
            .map_err(|_| TransportError::Protocol)?;
        if proof != session_proof(&inner, &connection)? {
            return Err(TransportError::Unauthorized);
        }
        let request = read(&mut recv, &inner.limits.protocol).await?;
        let permission = inner
            .permissions
            .get(&peer)
            .ok_or(TransportError::Unauthorized)?;
        match request {
            Message::OfferObjects(roots) if permission.objects => {
                drop(control);
                let bulk = inner.lease(peer, true)?;
                write(
                    &mut send,
                    &Message::WantObjects(roots.clone()),
                    &inner.limits.protocol,
                )
                .await?;
                let mut objects = Vec::new();
                let mut seen = BTreeSet::new();
                let mut current: Option<(TransferObject, usize)> = None;
                let mut bytes = 0usize;
                loop {
                    match read(&mut recv, &inner.limits.protocol).await? {
                        Message::ObjectChunk(chunk) => {
                            let total = usize::try_from(chunk.total_bytes)
                                .map_err(|_| TransportError::Limit)?;
                            if current.is_none() {
                                if chunk.offset != 0 || !seen.insert(chunk.id) {
                                    return Err(TransportError::InvalidGraph);
                                }
                                if seen.len() > inner.limits.protocol.max_ids {
                                    return Err(TransportError::Limit);
                                }
                                bytes = bytes.checked_add(total).ok_or(TransportError::Limit)?;
                                if bytes > inner.limits.max_batch_bytes {
                                    return Err(TransportError::Limit);
                                }
                                current = Some((
                                    TransferObject {
                                        id: chunk.id,
                                        kind: chunk.kind,
                                        schema: chunk.schema,
                                        bytes: Vec::with_capacity(total),
                                    },
                                    total,
                                ));
                            }
                            let (object, expected) =
                                current.as_mut().ok_or(TransportError::Protocol)?;
                            if object.id != chunk.id
                                || object.kind != chunk.kind
                                || object.schema != chunk.schema
                                || *expected != total
                                || object.bytes.len() as u64 != chunk.offset
                            {
                                return Err(TransportError::InvalidGraph);
                            }
                            object.bytes.extend_from_slice(&chunk.bytes);
                            if object.bytes.len() == total {
                                objects.push(current.take().ok_or(TransportError::Protocol)?.0);
                            }
                        }
                        Message::Ack(ack)
                            if !ack.durable && ack.objects == roots && current.is_none() =>
                        {
                            break;
                        }
                        _ => return Err(TransportError::Protocol),
                    }
                }
                let repository = inner.objects.clone();
                let scope = inner.scope;
                let roots_copy = roots.clone();
                let max = inner.limits.max_batch_bytes;
                // Cancellation cannot release the reservation while a synchronous
                // repository commit still owns the imported buffers.
                tokio::task::spawn_blocking(move || {
                    let _lease = bulk;
                    repository.import(scope, &roots_copy, objects, max)
                })
                .await
                .map_err(|_| TransportError::Storage)??;
                write(
                    &mut send,
                    &Message::Ack(Ack {
                        objects: roots,
                        durable: true,
                    }),
                    &inner.limits.protocol,
                )
                .await?;
                Ok(())
            }
            Message::BranchProposal(proposal) if permission.proposals => {
                if proposal.tenant != inner.scope.tenant || proposal.domain != inner.scope.domain {
                    return Err(TransportError::Unauthorized);
                }
                let id = proposal.request_id;
                let domain = proposal.domain;
                let epoch = proposal.epoch;
                let handler = inner.authority.clone();
                let context = ProposalContext {
                    peer,
                    tenant: inner.scope.tenant,
                    domain: inner.scope.domain,
                };
                let (reply, _lease) = tokio::task::spawn_blocking(move || {
                    handler
                        .propose(context, proposal)
                        .map(|reply| (reply, control))
                })
                .await
                .map_err(|_| TransportError::Application)??;
                if reply.request_id != id || reply.domain != domain || reply.epoch != epoch {
                    return Err(TransportError::Application);
                }
                write(
                    &mut send,
                    &Message::PromotionResult(reply),
                    &inner.limits.protocol,
                )
                .await?;
                Ok(())
            }
            Message::OfferObjects(_) | Message::BranchProposal(_) => {
                Err(TransportError::Unauthorized)
            }
            _ => Err(TransportError::Protocol),
        }
    }
    .await;
    if let Err(error) = &result {
        let code = match error {
            TransportError::Unauthorized => 1,
            TransportError::Limit => 2,
            TransportError::Storage => 3,
            TransportError::Busy => 4,
            TransportError::Application => 5,
            _ => 6,
        };
        let _ = write(
            &mut send,
            &Message::ProtocolError(ErrorMessage { code }),
            &inner.limits.protocol,
        )
        .await;
    }
    let _ = send.finish();
    // The initiator closes after receiving the durable response; do not reset a
    // stream immediately after queueing an acknowledgment.
    connection.closed().await;
    result
}
pub(crate) async fn push(
    inner: Arc<Inner>,
    connection: Connection,
    roots: Vec<ObjectId>,
    lease: Lease,
) -> Result<SyncReport, TransportError> {
    let _connection_lifetime = CloseOnDrop(connection.clone());
    let roots_copy = roots.clone();
    let repository = inner.objects.clone();
    let scope = inner.scope;
    let max = inner.limits.max_batch_bytes;
    let (objects, _lease) = tokio::task::spawn_blocking(move || {
        repository
            .export(scope, &roots_copy, max)
            .map(|objects| (objects, lease))
    })
    .await
    .map_err(|_| TransportError::Storage)??;
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|_| TransportError::Unavailable)?;
    // QUIC stream acceptance requires initiator bytes; write before waiting.
    initiate(&inner, &connection, &mut send, &mut recv).await?;
    write(
        &mut send,
        &Message::OfferObjects(roots.clone()),
        &inner.limits.protocol,
    )
    .await?;
    if read(&mut recv, &inner.limits.protocol).await? != Message::WantObjects(roots.clone()) {
        return Err(TransportError::Protocol);
    }
    let mut bytes = 0u64;
    let count = objects.len();
    let chunk_size = inner
        .limits
        .protocol
        .chunk_bytes
        .min(inner.limits.protocol.control_bytes - 128);
    for object in objects {
        bytes = bytes
            .checked_add(object.bytes.len() as u64)
            .ok_or(TransportError::Limit)?;
        for (index, part) in object.bytes.chunks(chunk_size).enumerate() {
            write(
                &mut send,
                &Message::ObjectChunk(Chunk {
                    id: object.id,
                    kind: object.kind,
                    schema: object.schema,
                    total_bytes: object.bytes.len() as u64,
                    offset: (index * chunk_size) as u64,
                    bytes: part.to_vec(),
                }),
                &inner.limits.protocol,
            )
            .await?;
        }
    }
    write(
        &mut send,
        &Message::Ack(Ack {
            objects: roots.clone(),
            durable: false,
        }),
        &inner.limits.protocol,
    )
    .await?;
    send.finish().map_err(|_| TransportError::Unavailable)?;
    let response = read(&mut recv, &inner.limits.protocol).await;
    connection.close(0u32.into(), b"done");
    if response?
        != Message::Ack(Ack {
            objects: roots.clone(),
            durable: true,
        })
    {
        return Err(TransportError::Protocol);
    }
    Ok(SyncReport {
        roots,
        objects: count,
        bytes,
        durable: true,
    })
}
pub(crate) async fn propose(
    inner: Arc<Inner>,
    connection: Connection,
    proposal: ProposalEnvelope,
    _lease: Lease,
) -> Result<PromotionReply, TransportError> {
    let _connection_lifetime = CloseOnDrop(connection.clone());
    let (mut send, mut recv) = connection
        .open_bi()
        .await
        .map_err(|_| TransportError::Unavailable)?;
    initiate(&inner, &connection, &mut send, &mut recv).await?;
    write(
        &mut send,
        &Message::BranchProposal(proposal.clone()),
        &inner.limits.protocol,
    )
    .await?;
    send.finish().map_err(|_| TransportError::Unavailable)?;
    let response = read(&mut recv, &inner.limits.protocol).await;
    connection.close(0u32.into(), b"done");
    match response? {
        Message::PromotionResult(reply)
            if reply.request_id == proposal.request_id
                && reply.domain == proposal.domain
                && reply.epoch == proposal.epoch =>
        {
            Ok(reply)
        }
        _ => Err(TransportError::Protocol),
    }
}
