use attow_protocol::*;
use chronos_codec::ObjectId;
use proptest::prelude::*;

fn id(n: u8) -> ObjectId {
    ObjectId([n; 32])
}
fn messages() -> Result<Vec<Message>, ProtocolError> {
    let mut checkpoint = Checkpoint {
        tenant: id(1),
        domain: id(2),
        writer: [0; 32],
        epoch: 3,
        sequence: 4,
        root: id(5),
        event_head: id(6),
        signature: [0; 64],
    };
    checkpoint.sign(&SigningKey::from_bytes(&[9; 32]))?;
    Ok(vec![
        Message::Hello(Hello {
            version: 1,
            tenant: id(1),
            domain: id(2),
        }),
        Message::OfferObjects(vec![id(1), id(2)]),
        Message::WantObjects(vec![id(3)]),
        Message::ObjectChunk(Chunk {
            id: id(4),
            kind: 1,
            schema: 1,
            total_bytes: 3,
            offset: 0,
            bytes: vec![1, 2, 3],
        }),
        Message::BranchProposal(ProposalEnvelope {
            request_id: id(1),
            tenant: id(2),
            domain: id(3),
            epoch: 4,
            branch: id(5),
            payload: id(6),
            decision: id(7),
            generation: 8,
        }),
        Message::PromotionResult(PromotionReply {
            request_id: id(1),
            domain: id(2),
            epoch: 3,
            status: PromotionStatus::AcceptedPending {
                seq: 4,
                intent: id(5),
            },
        }),
        Message::SignedCheckpoint(checkpoint),
        Message::Ack(Ack {
            objects: vec![id(1)],
            durable: true,
        }),
        Message::ProtocolError(ErrorMessage { code: 1 }),
    ])
}
#[test]
fn all_messages_roundtrip_and_bind_version() -> Result<(), ProtocolError> {
    assert_eq!(ALPN, b"attow/1");
    for message in messages()? {
        let bytes = encode_frame(&message)?;
        assert_eq!(
            u32::from_be_bytes(
                bytes[..4]
                    .try_into()
                    .map_err(|_| ProtocolError::Truncated)?
            ) as usize,
            bytes.len() - 4
        );
        assert_eq!(decode_frame(&bytes, &ProtocolLimits::default())?, message);
    }
    Ok(())
}
#[test]
fn hello_golden_frame() -> Result<(), ProtocolError> {
    let frame = encode_frame(&Message::Hello(Hello {
        version: 1,
        tenant: id(1),
        domain: id(2),
    }))?;
    let mut expected = vec![0, 0, 0, 71, 1, 0x83, 1, 0x58, 0x20];
    expected.extend([1; 32]);
    expected.extend([0x58, 0x20]);
    expected.extend([2; 32]);
    assert_eq!(frame, expected);
    Ok(())
}
#[test]
fn malformed_headers_rejected_before_payload_allocation() {
    let limits = ProtocolLimits::default();
    assert_eq!(
        decode_header(&[0xff, 0xff, 0xff, 0xff, 1], &limits),
        Err(ProtocolError::Limit)
    );
    assert_eq!(
        decode_header(&[0, 0, 0, 2, 255], &limits),
        Err(ProtocolError::UnknownMessage)
    );
    assert_eq!(
        decode_header(&[0, 0, 0, 0, 1], &limits),
        Err(ProtocolError::InvalidMessage)
    );
}
#[test]
fn strict_signature_binds_context_and_freshness() -> Result<(), ProtocolError> {
    let Message::SignedCheckpoint(checkpoint) = messages()?.remove(6) else {
        return Err(ProtocolError::InvalidMessage);
    };
    let trust = CheckpointTrust {
        tenant: checkpoint.tenant,
        domain: checkpoint.domain,
        writer: checkpoint.writer,
        epoch: 3,
        minimum_sequence: 4,
        expected_at_floor: Some((checkpoint.root, checkpoint.event_head)),
    };
    checkpoint.verify(&trust)?;
    let mut tampered = checkpoint.clone();
    tampered.root = id(99);
    assert_eq!(tampered.verify(&trust), Err(ProtocolError::Signature));
    let mut stale = trust.clone();
    stale.epoch = 4;
    assert_eq!(
        checkpoint.verify(&stale),
        Err(ProtocolError::StaleCheckpoint)
    );
    stale = trust.clone();
    stale.minimum_sequence = 5;
    assert_eq!(
        checkpoint.verify(&stale),
        Err(ProtocolError::StaleCheckpoint)
    );
    stale = trust;
    stale.writer = [8; 32];
    assert_eq!(checkpoint.verify(&stale), Err(ProtocolError::Trust));
    Ok(())
}

#[test]
fn truncated_noncanonical_and_unknown_version_frames_fail() -> Result<(), ProtocolError> {
    let frame = encode_frame(&Message::Hello(Hello {
        version: 1,
        tenant: id(1),
        domain: id(2),
    }))?;
    for length in 0..frame.len() {
        assert!(decode_frame(&frame[..length], &ProtocolLimits::default()).is_err());
    }
    let mut trailing = frame.clone();
    trailing.push(0);
    assert_eq!(
        decode_frame(&trailing, &ProtocolLimits::default()),
        Err(ProtocolError::TrailingBytes)
    );
    let mut version = frame.clone();
    version[6] = 2;
    assert_eq!(
        decode_frame(&version, &ProtocolLimits::default()),
        Err(ProtocolError::Version)
    );
    let mut noncanonical = frame;
    noncanonical[3] += 1;
    noncanonical.splice(6..7, [0x18, 1]);
    assert!(matches!(
        decode_frame(&noncanonical, &ProtocolLimits::default()),
        Err(ProtocolError::Codec(_))
    ));
    Ok(())
}

#[test]
fn chunk_bounds_and_duplicate_ids_fail_before_encoding() {
    let chunk = Chunk {
        id: id(1),
        kind: 1,
        schema: 1,
        total_bytes: MAX_OBJECT_BYTES + 1,
        offset: 0,
        bytes: vec![1],
    };
    assert_eq!(
        encode_frame(&Message::ObjectChunk(chunk)),
        Err(ProtocolError::Limit)
    );
    let chunk = Chunk {
        id: id(1),
        kind: 1,
        schema: 1,
        total_bytes: 3,
        offset: u64::MAX,
        bytes: vec![1],
    };
    assert!(encode_frame(&Message::ObjectChunk(chunk)).is_err());
    assert!(encode_frame(&Message::OfferObjects(vec![id(1), id(1)])).is_err());
    assert!(encode_frame(&Message::WantObjects(vec![id(1); MAX_IDS + 1])).is_err());
}

#[test]
fn checkpoint_rejects_weak_keys_malleability_and_equivocation() -> Result<(), ProtocolError> {
    let key = SigningKey::from_bytes(&[9; 32]);
    let Message::SignedCheckpoint(mut checkpoint) = messages()?.remove(6) else {
        return Err(ProtocolError::InvalidMessage);
    };
    let mut trust = CheckpointTrust {
        tenant: checkpoint.tenant,
        domain: checkpoint.domain,
        writer: checkpoint.writer,
        epoch: checkpoint.epoch,
        minimum_sequence: checkpoint.sequence,
        expected_at_floor: Some((checkpoint.root, checkpoint.event_head)),
    };
    let mut malleable = checkpoint.clone();
    malleable.signature[63] |= 0x80;
    assert_eq!(malleable.verify(&trust), Err(ProtocolError::Signature));
    checkpoint.root = id(99);
    checkpoint.sign(&key)?;
    assert_eq!(checkpoint.verify(&trust), Err(ProtocolError::Trust));
    checkpoint.sequence += 1;
    checkpoint.sign(&key)?;
    checkpoint.verify(&trust)?;
    checkpoint.writer = [0; 32];
    checkpoint.writer[0] = 1;
    checkpoint.signature = [0; 64];
    checkpoint.signature[0] = 1;
    trust.writer = checkpoint.writer;
    assert_eq!(checkpoint.verify(&trust), Err(ProtocolError::Signature));
    Ok(())
}

#[test]
fn restrictive_limits_apply_to_decoded_messages() -> Result<(), ProtocolError> {
    let chunk = Message::ObjectChunk(Chunk {
        id: id(1),
        kind: 1,
        schema: 1,
        total_bytes: MAX_OBJECT_BYTES,
        offset: MAX_OBJECT_BYTES - 4,
        bytes: vec![1; 4],
    });
    let frame = encode_frame(&chunk)?;
    let mut limits = ProtocolLimits {
        chunk_bytes: 3,
        ..ProtocolLimits::default()
    };
    assert_eq!(decode_frame(&frame, &limits), Err(ProtocolError::Limit));
    limits = ProtocolLimits::default();
    limits.object_bytes = 4;
    assert_eq!(decode_frame(&frame, &limits), Err(ProtocolError::Limit));
    limits = ProtocolLimits::default();
    limits.control_bytes = MAX_CONTROL_BYTES + 1;
    assert_eq!(
        decode_frame(&frame, &limits),
        Err(ProtocolError::InvalidLimits)
    );
    let offered = encode_frame(&Message::OfferObjects(vec![id(1), id(2)]))?;
    limits = ProtocolLimits::default();
    limits.max_ids = 1;
    assert_eq!(decode_frame(&offered, &limits), Err(ProtocolError::Limit));
    Ok(())
}

#[test]
fn all_result_variants_and_transfer_seal_are_distinct() -> Result<(), ProtocolError> {
    for status in [
        PromotionStatus::LocalCommitted { seq: 1 },
        PromotionStatus::Conflict {
            reasons: vec!["read-conflict".into()],
        },
    ] {
        let message = Message::PromotionResult(PromotionReply {
            request_id: id(1),
            domain: id(2),
            epoch: 1,
            status,
        });
        assert_eq!(
            decode_frame(&encode_frame(&message)?, &ProtocolLimits::default())?,
            message
        );
    }
    let seal = Message::Ack(Ack {
        objects: vec![id(1)],
        durable: false,
    });
    let committed = Message::Ack(Ack {
        objects: vec![id(1)],
        durable: true,
    });
    assert_ne!(encode_frame(&seal)?, encode_frame(&committed)?);
    Ok(())
}

#[test]
fn hostile_container_claims_and_deep_payloads_fail_with_fixed_small_frames() {
    let mut huge = vec![0, 0, 0, 12, 2, 0x82, 1, 0x9b];
    huge.extend([0xff; 8]);
    assert!(decode_frame(&huge, &ProtocolLimits::default()).is_err());
    let mut deep = vec![0, 0, 0, 35, 2];
    deep.extend([0x81; 33]);
    deep.push(1);
    assert!(matches!(
        decode_frame(&deep, &ProtocolLimits::default()),
        Err(ProtocolError::Codec(chronos_codec::CodecError::DepthLimit))
    ));
}

#[test]
fn maximum_chunk_and_chunk_header_bounds_are_exact() -> Result<(), ProtocolError> {
    let message = Message::ObjectChunk(Chunk {
        id: id(1),
        kind: 1,
        schema: 1,
        total_bytes: MAX_OBJECT_BYTES,
        offset: MAX_OBJECT_BYTES - MAX_CHUNK_BYTES as u64,
        bytes: vec![7; MAX_CHUNK_BYTES],
    });
    let bytes = encode_frame(&message)?;
    assert_eq!(decode_frame(&bytes, &ProtocolLimits::default())?, message);
    let claimed = (MAX_CHUNK_BYTES as u32 + 129).to_be_bytes();
    assert_eq!(
        decode_header(
            &[claimed[0], claimed[1], claimed[2], claimed[3], 4],
            &ProtocolLimits::default()
        ),
        Err(ProtocolError::Limit)
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]
    #[test]
    fn arbitrary_frames_are_bounded_and_accepted_bytes_are_canonical(bytes in proptest::collection::vec(any::<u8>(),0..8192)) {
        if let Ok(message)=decode_frame(&bytes,&ProtocolLimits::default()) {
            prop_assert_eq!(encode_frame(&message).expect("accepted frame must encode"),bytes);
        }
    }
    #[test]
    fn chunk_roundtrip_with_valid_ranges(data in proptest::collection::vec(any::<u8>(),1..2048),offset in 0..1024u64) {
        let message=Message::ObjectChunk(Chunk{id:id(1),kind:1,schema:1,total_bytes:offset+data.len() as u64,offset,bytes:data});
        let bytes=encode_frame(&message).expect("bounded chunk");
        prop_assert_eq!(decode_frame(&bytes,&ProtocolLimits::default()).expect("valid frame"),message);
    }
}
