use wasmtime::{StoreContextMut, StoreLimits, StoreLimitsBuilder, component::Linker};

use crate::{EffectClass, MAX_DATA_BYTES, SandboxError};

pub(crate) struct Host {
    pub inputs: [Vec<u8>; 3],
    pub outputs: [Vec<u8>; 4],
    pub limits: StoreLimits,
    output_limit: usize,
    output_len: usize,
    remaining_calls: u32,
    effect_class: Option<EffectClass>,
}
impl Host {
    pub fn new(
        inputs: [Vec<u8>; 3],
        memory: usize,
        output: usize,
        calls: u32,
        effect_class: Option<EffectClass>,
    ) -> Self {
        Self {
            inputs,
            outputs: std::array::from_fn(|_| Vec::new()),
            // Admission permits at most one memory, table and core instance;
            // this makes the per-memory byte limit an aggregate guest limit.
            limits: StoreLimitsBuilder::new()
                .memory_size(memory)
                .memories(1)
                .table_elements(1024)
                .tables(1)
                .instances(1)
                .trap_on_grow_failure(true)
                .build(),
            output_limit: output,
            output_len: 0,
            remaining_calls: calls,
            effect_class,
        }
    }
    fn charge(&mut self) -> Result<(), SandboxError> {
        self.remaining_calls = self
            .remaining_calls
            .checked_sub(1)
            .ok_or(SandboxError::HostCall)?;
        Ok(())
    }
    fn input(&self, source: u32) -> Result<&[u8], SandboxError> {
        self.inputs
            .get(source as usize)
            .map(Vec::as_slice)
            .ok_or(SandboxError::InvalidInput)
    }
    fn output(&mut self, target: u32, byte: u32) -> Result<(), SandboxError> {
        let request_denied = target == 2
            && self
                .effect_class
                .is_some_and(|class| class != EffectClass::PreparedExternal);
        let delta_denied = target == 3
            && self.effect_class.is_some_and(|class| {
                !matches!(
                    class,
                    EffectClass::ManagedWrite | EffectClass::PreparedExternal
                )
            });
        if byte > 255 || request_denied || delta_denied {
            return Err(SandboxError::InvalidInput);
        }
        let output = self
            .outputs
            .get_mut(target as usize)
            .ok_or(SandboxError::InvalidInput)?;
        if self.output_len >= self.output_limit || output.len() >= MAX_DATA_BYTES {
            return Err(SandboxError::Output);
        }
        // Reserve at most one bounded output stream; guest-controlled lengths
        // are never lifted into an unbounded host allocation.
        output
            .try_reserve(1)
            .map_err(|_| SandboxError::Limit("host allocation"))?;
        output.push(byte as u8);
        self.output_len += 1;
        Ok(())
    }
}

pub(crate) fn linker(engine: &wasmtime::Engine) -> Result<Linker<Host>, SandboxError> {
    let mut linker = Linker::<Host>::new(engine);
    linker
        .root()
        .func_wrap(
            "input-len",
            |mut store: StoreContextMut<'_, Host>, (source,): (u32,)| {
                let host = store.data_mut();
                host.charge()?;
                Ok((u32::try_from(host.input(source)?.len())
                    .map_err(|_| SandboxError::InvalidInput)?,))
            },
        )
        .map_err(|_| SandboxError::Admission)?;
    linker
        .root()
        .func_wrap(
            "input-byte",
            |mut store: StoreContextMut<'_, Host>, (source, index): (u32, u32)| {
                let host = store.data_mut();
                host.charge()?;
                let byte = host
                    .input(source)?
                    .get(index as usize)
                    .ok_or(SandboxError::InvalidInput)?;
                Ok((u32::from(*byte),))
            },
        )
        .map_err(|_| SandboxError::Admission)?;
    linker
        .root()
        .func_wrap(
            "output-byte",
            |mut store: StoreContextMut<'_, Host>, (target, byte): (u32, u32)| {
                let host = store.data_mut();
                host.charge()?;
                host.output(target, byte)?;
                Ok(())
            },
        )
        .map_err(|_| SandboxError::Admission)?;
    Ok(linker)
}

pub(crate) fn runtime_error(error: wasmtime::Error) -> SandboxError {
    if let Some(error) = error.downcast_ref::<SandboxError>() {
        return error.clone();
    }
    if matches!(
        error.downcast_ref::<wasmtime::Trap>(),
        Some(wasmtime::Trap::OutOfFuel)
    ) {
        SandboxError::Fuel
    } else {
        SandboxError::Trap
    }
}
