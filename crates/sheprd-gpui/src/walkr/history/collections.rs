//! Retention limits apply during decoding, before untrusted lists allocate.
use serde::{
    Deserialize, Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use std::{collections::VecDeque, fmt, marker::PhantomData};

pub(super) const ITEMS: usize = 64;

pub(super) struct First<T> {
    pub(super) items: Vec<T>,
    pub(super) omitted: usize,
}

impl<T> Default for First<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            omitted: 0,
        }
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for First<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FirstVisitor<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for FirstVisitor<T> {
            type Value = First<T>;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a list")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut kept = First::default();
                while kept.items.len() < ITEMS {
                    let Some(item) = seq.next_element()? else {
                        return Ok(kept);
                    };
                    kept.items.push(item);
                }
                while seq.next_element::<IgnoredAny>()?.is_some() {
                    kept.omitted += 1;
                }
                Ok(kept)
            }
        }
        deserializer.deserialize_seq(FirstVisitor(PhantomData))
    }
}

pub(super) fn comments<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<First<(String, String)>, D::Error> {
    struct Comments;
    impl<'de> Visitor<'de> for Comments {
        type Value = First<(String, String)>;
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("comments by option ID")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut kept = First::default();
            while kept.items.len() < ITEMS {
                let Some(entry) = map.next_entry()? else {
                    return Ok(kept);
                };
                kept.items.push(entry);
            }
            while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {
                kept.omitted += 1;
            }
            Ok(kept)
        }
    }
    deserializer.deserialize_map(Comments)
}

pub(super) struct Newest<T> {
    pub(super) items: VecDeque<T>,
    pub(super) total: usize,
}

impl<T> Default for Newest<T> {
    fn default() -> Self {
        Self {
            items: VecDeque::new(),
            total: 0,
        }
    }
}

pub(super) struct NewestSeed<T>(pub(super) usize, pub(super) PhantomData<T>);

impl<'de, T: Deserialize<'de>> DeserializeSeed<'de> for NewestSeed<T> {
    type Value = Newest<T>;
    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, T: Deserialize<'de>> Visitor<'de> for NewestSeed<T> {
    type Value = Newest<T>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("saved ratings")
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut kept = Newest::default();
        while let Some(item) = seq.next_element()? {
            kept.total += 1;
            if self.0 == 0 {
                continue;
            }
            if kept.items.len() == self.0 {
                kept.items.pop_front();
            }
            kept.items.push_back(item);
        }
        Ok(kept)
    }
}
