#[derive(Debug, Default)]
pub struct CorePlaceholder;

impl CorePlaceholder {
    pub fn name(&self) -> &'static str {
        "crossfeed-core"
    }
}

#[cfg(test)]
mod tests {
    use super::CorePlaceholder;

    #[test]
    fn reports_name() {
        let placeholder = CorePlaceholder::default();
        assert_eq!(placeholder.name(), "crossfeed-core");
    }
}
