use uuid::Uuid;

#[inline]
pub fn calculate_device_fingerprint() -> String {
    Uuid::new_v4().to_string()
}

#[inline]
pub fn default_fingerprint() -> String {
    "default-fingerprint".to_string()
}
