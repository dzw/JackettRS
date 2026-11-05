/// Mirror of Jackett.Common/Models/Config/ProxyType.cs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum ProxyType {
    Disabled = -1,
    Http = 0,
    Socks4 = 1,
    Socks5 = 2,
}

impl Default for ProxyType {
    fn default() -> Self {
        Self::Disabled
    }
}
