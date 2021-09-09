# scales - SCALE Serialization

Making use of [type information](https://github.com/paritytech/scale-info) this library allows for conversion of SCALE encoded data(wrapped in a `scales::Value`) to any format that implements `Serialize` including dynamic types like `serde_json::Value` for example. The opposite conversion of arbitrary data(e.g. JSON) to SCALE binary format is also possible with the `serializer` feature.
