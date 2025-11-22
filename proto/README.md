In order to not require users of this crate to have `protoc` installed, we check in a generated file
- `perfetto.protos.rs`.

If you need to change `perfetto_trace.proto` then you'll need to also regenerate
`perfetto.protos.rs`.

You can do this by running `./regenerate.rs`.

You'll need to have `protoc` installed. On apt-based systems, you can get it with: `sudo apt install
protobuf-compiler`.

