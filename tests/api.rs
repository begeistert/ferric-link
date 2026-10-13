use ferric_link::read_serial;

#[test]
fn read_serial_accepts_arrays_and_slices_and_reads_nothing() {
    let mut array = [0u8; 16];
    assert_eq!(read_serial!(&mut array), 0);
    let mut vec_backed = [7u8; 8];
    let slice: &mut [u8] = &mut vec_backed;
    assert_eq!(read_serial!(slice), 0);
    assert_eq!(vec_backed, [7u8; 8]);
}

#[test]
fn init_log_is_callable_repeatedly() {
    ferric_link::init_log();
    ferric_link::init_log();
}
