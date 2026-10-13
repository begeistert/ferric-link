//! Patched copies of two third-party USB classes, so that a project depending on
//! `ferric-link` needs no `[patch.crates-io]` section. See `README.md` here.

#[cfg(ferric_link_rp)]
pub mod usbd_ethernet;

#[cfg(feature = "embassy")]
pub mod embassy_ncm;
