# scales - SCALE Serialization

Making use of [type information](https://github.com/paritytech/scale-info) this library allows 
conversion to/from [SCALE](https://github.com/paritytech/parity-scale-codec) encoded data, 
specially useful when conversion is for dynamic types like JSON.

### From SCALE

`scales::Value` wraps the raw SCALE binary data and the type id within type registry 
giving you an object that can be serialized to any compatible format.

```rust
let value = scales::Value::new(scale_binary_data, type_id, &type_registry);
serde_json::to_string(value)?;
```

### To SCALE

Public methods from the `scales::serializer::*` module(feature `experimental-serializer`)
allow for a best effort conversion of dynamic types(e.g. `serde_json::Value`) to SCALE
binary format. The serializer tries to be smart when interpreting the input and convert it
to the desired format dictated by the provided type in the registry.

```rust
// simple conversion
let scale_data = to_vec(some_serializable_input); // or to_bytes(&mut bytes, input);

// with type info
let scale_data = to_vec_with_info(input, Some((&registry, type_id)));

// from an unordered list of properties that make an object
let input = vec![("prop_b", 123), ("prop_a", 456)];
let scale_data = to_vec_from_iter(input, (&registry, type_id));
```

