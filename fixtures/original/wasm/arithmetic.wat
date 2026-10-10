;; Original Attow Network test component, MIT OR Apache-2.0.
;; No WASI, network, filesystem, clock, randomness, or credentials.
(component
  (import "input-len" (func $input-len (param "source" u32) (result u32)))
  (import "input-byte" (func $input-byte (param "source" u32) (param "index" u32) (result u32)))
  (import "output-byte" (func $output-byte (param "target" u32) (param "byte" u32)))
  (core func $lower-len (canon lower (func $input-len)))
  (core func $lower-read (canon lower (func $input-byte)))
  (core func $lower-write (canon lower (func $output-byte)))
  (core module $guest
    (import "host" "input-len" (func $len (param i32) (result i32)))
    (import "host" "input-byte" (func $read (param i32 i32) (result i32)))
    (import "host" "output-byte" (func $write (param i32 i32)))
    (func (export "step") (result i32)
      i32.const 0
      i32.const 0 i32.const 0 call $read
      i32.const 1 i32.const 0 call $read
      i32.add call $write
      i32.const 0)

  )
  (core instance $host
    (export "input-len" (func $lower-len))
    (export "input-byte" (func $lower-read))
    (export "output-byte" (func $lower-write)))
  (core instance $run (instantiate $guest (with "host" (instance $host))))
  (func (export "step") (result u32) (canon lift (core func $run "step")))
)
