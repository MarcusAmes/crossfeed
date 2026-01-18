#[derive(Debug, Default)]
pub struct WebPlaceholder;

impl WebPlaceholder {
    pub fn name(&self) -> &'static str {
        "crossfeed-web"
    }
}

#[cfg(test)]
mod tests {
    use super::WebPlaceholder;

    #[test]
    fn reports_name() {
        let placeholder = WebPlaceholder::default();
        assert_eq!(placeholder.name(), "crossfeed-web");
    }
}
