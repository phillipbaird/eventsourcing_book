use std::path::PathBuf;

use fake::{
    Dummy, Fake,
    faker::filesystem::raw::FilePath,
    locales::EN,
    rand::seq::IteratorRandom,
    uuid::{UUIDv4, UUIDv7},
};
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::domain::cart::{AddItemCommand, AddItemPayload, CartId, ItemId, ProductId};

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

impl fake::Dummy<fake::Faker> for AddItemPayload {
    fn dummy_with_rng<R: ::fake::Rng + ?Sized>(_: &::fake::Faker, rng: &mut R) -> Self {
        let cart_id: Uuid = ::fake::Fake::fake_with_rng::<Uuid, _>(&(UUIDv7), rng);
        let description: String = ::fake::Fake::fake_with_rng::<String, _>(&::fake::Faker, rng);
        let image: String = ::fake::Fake::fake_with_rng::<String, _>(&(FilePath(EN)), rng);
        let price: Decimal = ::fake::Fake::fake_with_rng::<Decimal, _>(&(Price), rng);
        let item_id: Uuid = ::fake::Fake::fake_with_rng::<Uuid, _>(&(UUIDv7), rng);
        let product_id: Uuid = ::fake::Fake::fake_with_rng::<Uuid, _>(&(UUIDv7), rng);
        AddItemPayload {
            cart_id,
            description,
            image,
            price,
            item_id,
            product_id,
        }
    }
}

impl fake::Dummy<fake::Faker> for AddItemCommand {
    fn dummy_with_rng<R: ::fake::Rng + ?Sized>(_: &::fake::Faker, rng: &mut R) -> Self {
        let cart_id: CartId = ::fake::Fake::fake_with_rng::<CartId, _>(&::fake::Faker, rng);
        let description: String = ::fake::Fake::fake_with_rng::<String, _>(&::fake::Faker, rng);
        let image: PathBuf = ::fake::Fake::fake_with_rng::<PathBuf, _>(&::fake::Faker, rng);
        let price: Decimal = ::fake::Fake::fake_with_rng::<Decimal, _>(&(Price), rng);
        let item_id: ItemId = ::fake::Fake::fake_with_rng::<ItemId, _>(&::fake::Faker, rng);
        let product_id: ProductId =
            ::fake::Fake::fake_with_rng::<ProductId, _>(&::fake::Faker, rng);
        let fingerprint: String = ::fake::Fake::fake_with_rng::<String, _>(&(FingerPrint), rng);
        AddItemCommand {
            cart_id,
            description,
            image,
            price,
            item_id,
            product_id,
            fingerprint,
        }
    }
}
