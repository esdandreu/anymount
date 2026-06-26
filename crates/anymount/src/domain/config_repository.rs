use super::Config;

pub trait ConfigRepository {
    // TODO should it be a (String, Result<Config>)? Aknowledging that a mount
    // can be incorrectly configured.
    type Iter<'a>: Iterator<Item = (String, Config)>
    where
        Self: 'a;

    fn list(&self) -> Self::Iter<'_>;
}
