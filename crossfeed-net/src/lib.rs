#[derive(Debug, Default)]
pub struct NetPlaceholder;

impl NetPlaceholder {
    pub fn name(&self) -> &'static str {
        "crossfeed-net"
    }
}

#[cfg(test)]
mod tests {
    use super::NetPlaceholder;

    #[test]
    fn reports_name() {
        let placeholder = NetPlaceholder::default();
        assert_eq!(placeholder.name(), "crossfeed-net");
    }
}
