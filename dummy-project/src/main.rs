fn main() {
    println!("Hello, world!");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_addition() {
        // This test intentionally fails!
        // The agent should use `bash` to figure this out and rewrite it to `assert_eq!(2 + 2, 4);`
        assert_eq!(2 + 2, 5);
    }
}
