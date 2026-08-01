const PLACEHOLDER_OUTPUT: &str = "weavelit-server placeholder";

fn main() {
    println!("{PLACEHOLDER_OUTPUT}");
}

#[cfg(test)]
mod tests {
    use super::PLACEHOLDER_OUTPUT;

    #[test]
    fn reports_placeholder_output() {
        // Temporary executable-spine test; remove it with the placeholder startup code.
        assert_eq!(PLACEHOLDER_OUTPUT, "weavelit-server placeholder");
    }
}
