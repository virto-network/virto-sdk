use serde::{de::SeqAccess, Deserialize};
use virto_sdk::authenticator::AuthError;
use wasm_bindgen::JsError;

use core::marker::PhantomData;

pub(crate) struct SeqIter<'de, A, T> {
    access: A,
    marker: PhantomData<(&'de (), T)>,
}

impl<'de, A, T> SeqIter<'de, A, T> {
    pub(crate) fn new(access: A) -> Self
    where
        A: SeqAccess<'de>,
    {
        Self {
            access,
            marker: PhantomData,
        }
    }
}

impl<'de, A, T> Iterator for SeqIter<'de, A, T>
where
    A: SeqAccess<'de>,
    T: Deserialize<'de>,
{
    type Item = Result<T, A::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        self.access.next_element().transpose()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self.access.size_hint() {
            Some(size) => (size, Some(size)),
            None => (0, None),
        }
    }
}

pub mod signing_algorithm {
    pub const EDSA: i32 = -7;
    pub const RSA: i32 = -257;
}
