# Vendored USB classes

Two third-party CDC-NCM (Ethernet over USB) classes, copied here with one fix each. A published crate cannot impose a
`[patch.crates-io]` on the projects that depend on it, so the patched code ships inside it.

| Directory        | Upstream                                                   | Version | License            |
|------------------|------------------------------------------------------------|---------|--------------------|
| `usbd_ethernet/` | <https://github.com/dlkj/usbd-ethernet> (`usbd-ethernet`)  | 0.4.0   | MIT (`LICENSE-MIT`) |
| `embassy_ncm/`   | <https://github.com/embassy-rs/embassy> (`embassy-usb`, `src/class/cdc_ncm`) | 0.6.0 | MIT OR Apache-2.0 (`LICENSE-MIT`) |

`usbd_ethernet` is used by the `rp2040`, `rp2040-hal-0-10` and `rp235x` flavours, `embassy_ncm` by `embassy`.

## The bug

Apple bug report r.170072016 (macOS 26 / iPadOS 26): when a CDC-NCM function's `bmNetworkCapabilities` byte is `0`
(no capabilities declared), these hosts leave the NCM **data** interface disabled after enumeration and never
re-enable it. The device enumerates as "Ethernet" in Settings, but no packet crosses. Declaring the
`SetEthernetPacketFilter` capability (bit `D0`) and accepting that control request (`0x43`) avoids it. This is the fix
embassy applied upstream in [embassy-rs/embassy#6844](https://github.com/embassy-rs/embassy/pull/6844) (commit
`f12cb393`), which is newer than the `embassy-usb` 0.6.0 release. `usbd-ethernet` has the identical bug.

## What changed against the originals

Both classes:

- `bmNetworkCapabilities` `0x00` -> `0x01`, and `SET_ETHERNET_PACKET_FILTER` (`0x43`) is accepted instead of left unhandled.

`usbd_ethernet` only:

- the `defmt` logging and derives are removed. Upstream's manifest forces `usb-device`'s and `smoltcp`'s `defmt`
  features on, which obliges the final firmware to link a `defmt` global logger; without them none is needed;
- the MAC address string is a `[u8; 12]` instead of a `heapless::String<12>` (one dependency fewer);
- the `mod.rs` that ties the three files together replaces `lib.rs`.

`embassy_ncm` only:

- `crate::` paths point at `embassy_usb::`, and the `info!` / `warn!` / `trace!` logging macros are no-ops here;
- no other change (`mod.rs`, `embassy_net.rs`).

Remove this directory when `usbd-ethernet` and `embassy-usb` release the fix.
