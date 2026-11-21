# instax-rs

Rust port of [jpwsutton/instax_api](https://github.com/jpwsutton/instax_api) (MIT) for the
Fujifilm Instax SP-2 and SP-3, talking to the printer over its own Wi-Fi network (TCP 8080).

## Build and test

    cargo test
    cargo build --release

Use a normal internet connection for this; the printer's Wi-Fi has no internet.

## Use (join the `INSTAX-xxxxxxxx` Wi-Fi first)

    instax-rs info                          # read-only, safe
    instax-rs preview photo.jpg out.png     # convert only, no network
    instax-rs print photo.jpg --dry-run     # encode only, no network
    instax-rs print photo.jpg               # USES ONE SHEET OF FILM

Options: `-m 2|3` model (default 3), `-i` PIN (default 1111), `--host`, `--port`.
Run `info` first. `print` also refuses to start if the printer reports a different
model (`--force` overrides) or 0 prints remaining.

## Layout

- `src/packet.rs`   wire format, checksum, response decoding (std only, unit-tested)
- `src/image_enc.rs` EXIF rotate, crop-to-fill, 90° rotate, column-planar RGB encoding
- `src/client.rs`   blocking TCP request/response
- `src/printer.rs`  info + print sequence (pre-print x8, lock, reset, image chunks of 60000 B, poll)

## Differences from the Python original

- Aborts on fault return codes (film empty, wrong PIN, battery empty, ...); Python ignores them.
- Transparent pixels are composited onto white (Python's `paste` drops alpha).
- Model check and film check before printing; `--dry-run`, `preview` and `info` modes.
- SP-1 is not supported (it isn't implemented upstream either).
- No debug server / replay tests.
