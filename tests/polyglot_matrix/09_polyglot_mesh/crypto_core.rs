pub fn sign_checksum(value: i32, salt: i32) -> i32 {
    (value ^ salt) + 7
}
