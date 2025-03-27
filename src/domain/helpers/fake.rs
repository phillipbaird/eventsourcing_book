use fake::{rand::seq::IteratorRandom, uuid::UUIDv4, Dummy, Fake};
use rust_decimal::Decimal;
use uuid::Uuid;

pub struct Price;

impl Dummy<Price> for Decimal {
    fn dummy_with_rng<R: fake::Rng + ?Sized>(_config: &Price, rng: &mut R) -> Self {
        let value = (10..1000).choose(rng).unwrap();
        Decimal::new(value, 2)
    }
}

pub struct FingerPrint;

impl Dummy<FingerPrint> for String {
    fn dummy_with_rng<R: fake::Rng + ?Sized>(_config: &FingerPrint, rng: &mut R) -> Self {
        let uuid: Uuid = UUIDv4.fake_with_rng(rng);
        uuid.to_string()
    }
}
