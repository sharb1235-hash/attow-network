//! Structural admission precedes local compilation of bounded component binaries.
use crate::{SandboxError, profile::*};
use wasmparser::{
    BinaryReader, CanonicalFunction, ComponentExternalKind, ComponentTypeRef, ComponentValType,
    Encoding, Instance, Parser, Payload, PrimitiveValType, RefType, TypeRef, ValType,
};
use wasmtime::{
    Config, Engine, PoolingAllocationConfig, WasmFeatures,
    component::{
        Component,
        types::{ComponentItem, Type},
    },
};

pub(crate) fn engine() -> Result<Engine, SandboxError> {
    let mut config = Config::new();
    // Explicit whitelist also excludes threads without calling the Cargo-gated
    // wasm_threads method. Future component features start disabled.
    config
        .wasm_features(WasmFeatures::all(), false)
        .wasm_features(supported_features(), true)
        .consume_fuel(true)
        .cranelift_nan_canonicalization(true)
        .max_wasm_stack(MAX_WASM_STACK)
        .memory_init_cow(false)
        .memory_reservation(MAX_MEMORY_BYTES as u64)
        .memory_reservation_for_growth(0)
        .memory_may_move(false);
    let mut pool = PoolingAllocationConfig::default();
    pool.total_component_instances(MAX_WORKERS)
        .total_core_instances(MAX_WORKERS)
        .total_memories(MAX_WORKERS)
        .total_tables(MAX_WORKERS)
        .max_core_instances_per_component(1)
        .max_memories_per_component(1)
        .max_tables_per_component(1)
        .max_memories_per_module(1)
        .max_tables_per_module(1)
        .max_memory_size(MAX_MEMORY_BYTES)
        .table_elements(MAX_TABLE_ELEMENTS)
        .max_core_instance_size(MAX_CORE_INSTANCE_BYTES)
        .max_component_instance_size(MAX_COMPONENT_INSTANCE_BYTES);
    config.allocation_strategy(pool);
    Engine::new(&config).map_err(|_| SandboxError::Admission)
}

fn parse<T>(result: wasmparser::Result<T>) -> Result<T, SandboxError> {
    result.map_err(|_| SandboxError::Admission)
}

fn bounded(value: u32, max: u32) -> Result<(), SandboxError> {
    if value > max {
        Err(SandboxError::Limit("component structure"))
    } else {
        Ok(())
    }
}

fn add(total: &mut u32, amount: u32, max: u32) -> Result<(), SandboxError> {
    *total = total
        .checked_add(amount)
        .ok_or(SandboxError::Limit("component structure"))?;
    bounded(*total, max)
}

fn single(total: &mut u32, amount: u32) -> Result<(), SandboxError> {
    *total = total.checked_add(amount).ok_or(SandboxError::Admission)?;
    if *total > 1 {
        Err(SandboxError::Admission)
    } else {
        Ok(())
    }
}

fn allowed_import(name: &str) -> bool {
    matches!(name, "input-len" | "input-byte" | "output-byte")
}

// Inspect only supported flat function encodings before invoking recursive type
// readers. An excluded deeply nested component/GC type cannot consume the host
// stack or allocate its claimed recursive structure just to be rejected later.
fn flat_types(
    wasm: &[u8],
    start: usize,
    end: usize,
    count: u32,
    component: bool,
) -> Result<(), SandboxError> {
    let bytes = wasm.get(start..end).ok_or(SandboxError::Admission)?;
    let mut reader = BinaryReader::new(bytes, start);
    for _ in 0..count {
        if parse(reader.read_u8())? != if component { 0x40 } else { 0x60 } {
            return Err(SandboxError::Admission);
        }
        let params = parse(reader.read_var_u32())?;
        bounded(params, if component { 2 } else { 32 })?;
        for _ in 0..params {
            if component {
                if parse(reader.read_string())?.len() > 64 {
                    return Err(SandboxError::Admission);
                }
                if parse(reader.read::<ComponentValType>())?
                    != ComponentValType::Primitive(PrimitiveValType::U32)
                {
                    return Err(SandboxError::Admission);
                }
            } else if !matches!(
                parse(reader.read::<ValType>())?,
                ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64
            ) {
                return Err(SandboxError::Admission);
            }
        }
        if component {
            match parse(reader.read_u8())? {
                0 => {
                    if parse(reader.read::<ComponentValType>())?
                        != ComponentValType::Primitive(PrimitiveValType::U32)
                    {
                        return Err(SandboxError::Admission);
                    }
                }
                1 => {
                    if parse(reader.read_u8())? != 0 {
                        return Err(SandboxError::Admission);
                    }
                }
                _ => return Err(SandboxError::Admission),
            }
        } else {
            let results = parse(reader.read_var_u32())?;
            bounded(results, 8)?;
            for _ in 0..results {
                if !matches!(
                    parse(reader.read::<ValType>())?,
                    ValType::I32 | ValType::I64 | ValType::F32 | ValType::F64
                ) {
                    return Err(SandboxError::Admission);
                }
            }
        }
    }
    if !reader.eof() {
        return Err(SandboxError::Admission);
    }
    Ok(())
}

pub(crate) fn inspect(wasm: &[u8]) -> Result<(), SandboxError> {
    if wasm.len() > MAX_CODE_BYTES {
        return Err(SandboxError::Limit("component bytes"));
    }
    if !wasm.starts_with(b"\0asm\x0d\0\x01\0") {
        return Err(SandboxError::Admission);
    }
    let (
        mut sections,
        mut modules,
        mut instances,
        mut bundles,
        mut memories,
        mut tables,
        mut types,
        mut locals,
        mut operators,
        mut imports,
        mut exports,
    ) = (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0);
    for payload in Parser::new(0).parse_all(wasm) {
        add(&mut sections, 1, MAX_SECTIONS)?;
        match parse(payload)? {
            Payload::Version { encoding, .. } => {
                if (sections == 1 && encoding != Encoding::Component)
                    || (sections != 1 && encoding != Encoding::Module)
                {
                    return Err(SandboxError::Admission);
                }
            }
            Payload::ModuleSection { .. } => single(&mut modules, 1)?,
            Payload::TypeSection(reader) => {
                add(&mut types, reader.count(), MAX_TYPES)?;
                flat_types(
                    wasm,
                    reader.original_position(),
                    reader.range().end,
                    reader.count(),
                    false,
                )?;
            }
            Payload::ComponentTypeSection(reader) => {
                add(&mut types, reader.count(), MAX_TYPES)?;
                flat_types(
                    wasm,
                    reader.original_position(),
                    reader.range().end,
                    reader.count(),
                    true,
                )?;
            }
            Payload::ImportSection(reader) => {
                bounded(reader.count(), 3)?;
                let mut count = 0;
                for import in reader.into_imports() {
                    add(&mut count, 1, 3)?;
                    let import = parse(import)?;
                    if import.module != "host"
                        || !allowed_import(import.name)
                        || !matches!(import.ty, TypeRef::Func(_))
                    {
                        return Err(SandboxError::Admission);
                    }
                }
            }
            Payload::ComponentImportSection(reader) => {
                add(&mut imports, reader.count(), 3)?;
                for import in reader {
                    let import = parse(import)?;
                    if !allowed_import(import.name.name)
                        || !matches!(import.ty, ComponentTypeRef::Func(_))
                    {
                        return Err(SandboxError::Admission);
                    }
                }
            }
            Payload::FunctionSection(reader) => bounded(reader.count(), MAX_FUNCTIONS)?,
            Payload::CodeSectionStart { count, .. } => bounded(count, MAX_FUNCTIONS)?,
            Payload::CodeSectionEntry(body) => {
                for local in parse(body.get_locals_reader())? {
                    add(&mut locals, parse(local)?.0, MAX_LOCALS)?;
                }
                let mut reader = parse(body.get_operators_reader())?;
                while !reader.eof() {
                    parse(reader.read())?;
                    add(&mut operators, 1, MAX_OPERATORS)?;
                }
            }
            Payload::MemorySection(reader) => {
                single(&mut memories, reader.count())?;
                for memory in reader {
                    let memory = parse(memory)?;
                    if memory.memory64
                        || memory.shared
                        || memory.page_size_log2.is_some()
                        || memory.maximum != Some(memory.initial)
                        || memory.initial > MAX_MEMORY_BYTES as u64 / 65_536
                    {
                        return Err(SandboxError::Admission);
                    }
                }
            }
            Payload::TableSection(reader) => {
                single(&mut tables, reader.count())?;
                for table in reader {
                    let table = parse(table)?.ty;
                    if table.table64
                        || table.shared
                        || table.element_type != RefType::FUNCREF
                        || table.maximum != Some(table.initial)
                        || table.initial > MAX_TABLE_ELEMENTS as u64
                    {
                        return Err(SandboxError::Admission);
                    }
                }
            }
            Payload::GlobalSection(reader) => {
                bounded(reader.count(), MAX_AUX_ITEMS)?;
                for global in reader {
                    let global = parse(global)?;
                    if global.ty.mutable || global.ty.shared {
                        return Err(SandboxError::Admission);
                    }
                }
            }
            Payload::InstanceSection(reader) => {
                bounded(reader.count(), 2)?;
                for instance in reader {
                    match parse(instance)? {
                        Instance::Instantiate { args, .. } => {
                            single(&mut instances, 1)?;
                            if args.len() > 1 {
                                return Err(SandboxError::Admission);
                            }
                        }
                        Instance::FromExports(items) => {
                            single(&mut bundles, 1)?;
                            if items.len() > 3 {
                                return Err(SandboxError::Admission);
                            }
                        }
                    }
                }
            }
            Payload::ComponentCanonicalSection(reader) => {
                bounded(reader.count(), MAX_AUX_ITEMS)?;
                for function in reader {
                    match parse(function)? {
                        CanonicalFunction::Lift { options, .. }
                        | CanonicalFunction::Lower { options, .. }
                            if options.is_empty() => {}
                        _ => return Err(SandboxError::Admission),
                    }
                }
            }
            Payload::ComponentAliasSection(reader) => bounded(reader.count(), MAX_AUX_ITEMS)?,
            Payload::ComponentExportSection(reader) => {
                single(&mut exports, reader.count())?;
                for export in reader {
                    let export = parse(export)?;
                    if export.name.name != "step" || export.kind != ComponentExternalKind::Func {
                        return Err(SandboxError::Admission);
                    }
                }
            }
            Payload::ExportSection(reader) => bounded(reader.count(), MAX_AUX_ITEMS)?,
            Payload::ElementSection(reader) => bounded(reader.count(), MAX_AUX_ITEMS)?,
            Payload::DataSection(reader) => bounded(reader.count(), MAX_AUX_ITEMS)?,
            Payload::DataCountSection { count, .. } => bounded(count, MAX_AUX_ITEMS)?,
            Payload::CustomSection(_) | Payload::End(_) => {}
            _ => return Err(SandboxError::Admission),
        }
    }
    if modules != 1 || instances != 1 || exports != 1 {
        return Err(SandboxError::Admission);
    }
    Ok(())
}

pub(crate) fn compile(engine: &Engine, wasm: &[u8]) -> Result<Component, SandboxError> {
    inspect(wasm)?;
    let component = Component::new(engine, wasm).map_err(|_| SandboxError::Admission)?;
    let ty = component.component_type();
    for (name, item) in ty.imports(engine) {
        let ComponentItem::ComponentFunc(function) = item.ty else {
            return Err(SandboxError::Admission);
        };
        let (params, results) = match name {
            "input-len" => (1, 1),
            "input-byte" => (2, 1),
            "output-byte" => (2, 0),
            _ => return Err(SandboxError::Admission),
        };
        if function.params().len() != params
            || function.results().len() != results
            || !function.params().all(|(_, ty)| ty == Type::U32)
            || !function.results().all(|ty| ty == Type::U32)
        {
            return Err(SandboxError::Admission);
        }
    }
    let item = ty
        .get_export(engine, "step")
        .ok_or(SandboxError::Admission)?;
    let ComponentItem::ComponentFunc(function) = item.ty else {
        return Err(SandboxError::Admission);
    };
    if function.params().len() != 0 || function.results().collect::<Vec<_>>() != [Type::U32] {
        return Err(SandboxError::Admission);
    }
    Ok(component)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn component(extra: &str) -> Vec<u8> {
        wat::parse_str(format!(
            r#"(component
            (core module $m {extra} (func (export "step") (result i32) i32.const 0))
            (core instance $i (instantiate $m))
            (func (export "step") (result u32) (canon lift (core func $i "step"))))"#
        ))
        .expect("wat")
    }

    #[test]
    fn validates_real_component_and_denies_unbounded_or_hidden_state() {
        assert!(inspect(&component("(memory 1 1)")).is_ok());
        for extra in [
            "(memory 1)",
            "(memory 1 2)",
            "(memory 257 257)",
            "(memory 1 1) (memory 1 1)",
            "(global (mut i32) (i32.const 0))",
            "(table 1 funcref)",
            "(table 1025 1025 funcref)",
        ] {
            assert!(inspect(&component(extra)).is_err(), "accepted {extra}");
        }
    }

    #[test]
    fn rejects_malformed_text_core_modules_and_structural_bombs() {
        for bytes in [
            b"(component)".to_vec(),
            vec![],
            b"\0asm\x01\0\0\0".to_vec(),
            vec![0; crate::profile::MAX_CODE_BYTES + 1],
        ] {
            assert!(inspect(&bytes).is_err());
        }
        assert!(inspect(&component("(func (local i32 i32))")).is_ok());
        assert!(
            inspect(&component(&format!(
                "(func {})",
                "(local i32)".repeat(4097)
            )))
            .is_err()
        );
    }

    #[test]
    fn component_engine_rejects_ambient_imports_and_simd() {
        let engine = engine().expect("engine");
        assert!(compile(&engine, &component("")).is_ok());
        let denied =
            wat::parse_str(r#"(component (import "wasi:clocks/wall-clock@0.2.0" (instance)))"#)
                .expect("wat");
        assert!(compile(&engine, &denied).is_err());
        assert!(
            compile(
                &engine,
                &component("(func (drop (v128.const i32x4 0 0 0 0)))")
            )
            .is_err()
        );
    }
}
