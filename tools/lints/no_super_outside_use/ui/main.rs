mod parent {
    pub struct Thing;
}

mod child {
    use super::parent::Thing;

    fn imported_type(_: Thing) {}

    fn direct_type(_: super::parent::Thing) {}

    fn direct_value() {
        let _ = super::parent::Thing;
    }

    fn direct_pattern(value: super::parent::Thing) {
        let super::parent::Thing = value;
    }
}

fn main() {}
