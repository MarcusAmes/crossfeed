#[derive(Debug, Default)]
pub struct StoragePlaceholder;

impl StoragePlaceholder {
    pub fn name(&self) -> &'static str {
        "crossfeed-storage"
    }
}

#[cfg(test)]
mod tests {
    use super::StoragePlaceholder;

    #[test]
    fn reports_name() {
        let placeholder = StoragePlaceholder::default();
        assert_eq!(placeholder.name(), "crossfeed-storage");
    }
}
