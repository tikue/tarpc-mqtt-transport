use std::convert::TryInto;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use cipher::BlockEncrypt;
use cipher::generic_array::GenericArray;
use rc5::{RC5_32_12_16};
use tarpc::client::RequestSequencer;

/// Uses a ciphersuite to generate a unique permutation of the u64 nunbers to reduce the chance of collisions
pub struct MqttRequestSequencer {
    next: Arc<AtomicU64>,
    key: u128,
    cipher: RC5_32_12_16
}

impl Clone for MqttRequestSequencer {
    fn clone(&self) -> Self {
        MqttRequestSequencer::new(self.next.load(Ordering::Relaxed), self.key)
    }
}

impl Debug for MqttRequestSequencer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f
            .debug_struct("MqttRequestSequencer")
            .field("next", &self.next)
            .field("key", &self.key)
            .finish()
    }
}

impl MqttRequestSequencer {
    pub fn new(start :u64, key: u128) -> MqttRequestSequencer {
        let cipher = <RC5_32_12_16 as cipher::KeyInit>::new_from_slice(&key.to_le_bytes()).unwrap();

        MqttRequestSequencer {
            next: Arc::new(AtomicU64::new(start)),
            key,
            cipher
        }

    }

    pub fn from_zero_with_key(key: u128) -> MqttRequestSequencer {
        Self::new(0, key)
    }

    pub fn random() -> MqttRequestSequencer {
        Self::new(rand::random(), rand::random())
    }
}

impl RequestSequencer for MqttRequestSequencer {
    fn next_id(&self) -> u64 {

        let next_id = self.next.fetch_add(291381, Ordering::Relaxed);

        let mut block = GenericArray::clone_from_slice(&next_id.to_le_bytes());
        self.cipher.encrypt_block(&mut block);

        u64::from_le_bytes(block.as_slice().try_into().expect("block is not 8 bytes"))
    }
}
