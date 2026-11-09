pub fn checksum(val: i32) -> i32 {
    let mask = 23130;
    return val ^ mask;
}
