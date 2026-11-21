use std::{cell::Cell, rc::Rc};

use anymount::{ConfigRepository, LoadedConfig};

#[derive(Clone, Default)]
pub struct EmptyConfigRepository {
    reads: Rc<Cell<usize>>,
}

impl EmptyConfigRepository {
    pub fn reads(&self) -> usize {
        self.reads.get()
    }
}

impl ConfigRepository for EmptyConfigRepository {
    type Iter<'a> = std::iter::Empty<LoadedConfig>;

    fn list(&self) -> Self::Iter<'_> {
        self.reads.set(self.reads.get() + 1);
        std::iter::empty()
    }
}
