pub fn compute_luhn(num: i64) -> i64 {
    let mut sum: i64 = 0
    let mut temp: i64 = num
    let mut is_second: i64 = 0
    while temp > 0 {
        let mut d = temp % 10
        if is_second == 1 {
            d = d * 2
            if d > 9 {
                d = d - 9
            }
            is_second = 0
        } else {
            is_second = 1
        }
        sum = sum + d
        temp = temp / 10
    }
    return sum % 10
}
