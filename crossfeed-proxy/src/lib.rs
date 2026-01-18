#[derive(Debug, Default)]
pub struct ProxyPlaceholder;

impl ProxyPlaceholder {
    pub fn name(&self) -> &'static str {
        "crossfeed-proxy"
    }
}

#[cfg(test)]
mod tests {
    use super::ProxyPlaceholder;

    #[test]
    fn reports_name() {
        let placeholder = ProxyPlaceholder::default();
        assert_eq!(placeholder.name(), "crossfeed-proxy");
    }
}
