//! Mirrors of the engine's values for UniFFI (the engine and core types stay free of any UniFFI attribute).
//!
//! Each mirror has the fields of its engine type, in the same order, with the same names, and two conversions through the
//! [`ToMirror`] and [`ToEngine`] traits. A mirror that the host never sends has only the forward conversion. The macros
//! below write the bulk of a mirror; a type with private fields or a normalising constructor is written by hand.

// A mirror enum is built once and moved once across the boundary; boxing its large variants would only change how the plain
// data of the Swift enum is spelled (the engine's own enums allow the same lint for the same reason).
#![allow(clippy::large_enum_variant)]

pub mod accounts;
pub mod commands;
pub mod diagnostics;
pub mod events;
pub mod host;
pub mod model;
pub mod settings;
pub mod state;
pub mod summary;
pub mod widget;
pub mod write;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Converts an engine value into its UniFFI mirror (engine to host).
pub trait ToMirror {
    /// The mirror type.
    type Mirror;
    /// Converts, consuming the engine value.
    fn to_mirror(self) -> Self::Mirror;
}

/// Converts a mirror back into the engine value (host to engine).
pub trait ToEngine {
    /// The engine type.
    type Engine;
    /// Converts, consuming the mirror.
    fn to_engine(self) -> Self::Engine;
}

/// Values that cross unchanged: the primitives and the custom types of `custom_types`.
macro_rules! identity_mirror {
    ($($t:ty),* $(,)?) => {$(
        impl ToMirror for $t { type Mirror = $t; fn to_mirror(self) -> $t { self } }
        impl ToEngine for $t { type Engine = $t; fn to_engine(self) -> $t { self } }
    )*};
}
identity_mirror!(
    bool,
    u8,
    u16,
    u32,
    u64,
    i32,
    i64,
    f64,
    String,
    jiff::Timestamp,
    calendar_core::time::Zone,
    calendar_core::url_string::UrlString,
    uuid::Uuid,
);

impl<T: ToMirror> ToMirror for Vec<T> {
    type Mirror = Vec<T::Mirror>;
    fn to_mirror(self) -> Vec<T::Mirror> {
        self.into_iter().map(ToMirror::to_mirror).collect()
    }
}
impl<T: ToEngine> ToEngine for Vec<T> {
    type Engine = Vec<T::Engine>;
    fn to_engine(self) -> Vec<T::Engine> {
        self.into_iter().map(ToEngine::to_engine).collect()
    }
}
impl<T: ToMirror> ToMirror for Option<T> {
    type Mirror = Option<T::Mirror>;
    fn to_mirror(self) -> Option<T::Mirror> {
        self.map(ToMirror::to_mirror)
    }
}
impl<T: ToEngine> ToEngine for Option<T> {
    type Engine = Option<T::Engine>;
    fn to_engine(self) -> Option<T::Engine> {
        self.map(ToEngine::to_engine)
    }
}
impl<T: ToMirror> ToMirror for Box<T> {
    type Mirror = Box<T::Mirror>;
    fn to_mirror(self) -> Box<T::Mirror> {
        Box::new((*self).to_mirror())
    }
}
impl<T: ToEngine> ToEngine for Box<T> {
    type Engine = Box<T::Engine>;
    fn to_engine(self) -> Box<T::Engine> {
        Box::new((*self).to_engine())
    }
}
/// UniFFI has no ordered sets or maps: `BTreeSet` crosses as `HashSet` and `BTreeMap` as `HashMap` (Swift `Set` and
/// `Dictionary` are unordered too). Where an order is observable the engine type is a `Vec` already.
impl<T> ToMirror for BTreeSet<T>
where
    T: ToMirror,
    T::Mirror: std::hash::Hash + Eq,
{
    type Mirror = HashSet<T::Mirror>;
    fn to_mirror(self) -> HashSet<T::Mirror> {
        self.into_iter().map(ToMirror::to_mirror).collect()
    }
}
impl<T> ToEngine for HashSet<T>
where
    T: ToEngine,
    T::Engine: Ord,
{
    type Engine = BTreeSet<T::Engine>;
    fn to_engine(self) -> BTreeSet<T::Engine> {
        self.into_iter().map(ToEngine::to_engine).collect()
    }
}
impl<K, V> ToMirror for BTreeMap<K, V>
where
    K: ToMirror,
    K::Mirror: std::hash::Hash + Eq,
    V: ToMirror,
{
    type Mirror = HashMap<K::Mirror, V::Mirror>;
    fn to_mirror(self) -> HashMap<K::Mirror, V::Mirror> {
        self.into_iter()
            .map(|(key, value)| (key.to_mirror(), value.to_mirror()))
            .collect()
    }
}
impl<K, V> ToEngine for HashMap<K, V>
where
    K: ToEngine,
    K::Engine: Ord,
    V: ToEngine,
{
    type Engine = BTreeMap<K::Engine, V::Engine>;
    fn to_engine(self) -> BTreeMap<K::Engine, V::Engine> {
        self.into_iter()
            .map(|(key, value)| (key.to_engine(), value.to_engine()))
            .collect()
    }
}

/// A struct mirror with public fields only: the same fields in the same order, converted field by field.
/// `forward_only` leaves out the host-to-engine conversion.
macro_rules! mirror_record {
    (
        $(#[$meta:meta])*
        $name:ident <=> $engine:path {
            $( $(#[$fmeta:meta])* $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        mirror_record!(@type $(#[$meta])* $name { $( $(#[$fmeta])* $field : $ty ),* });
        impl crate::values::ToEngine for $name {
            type Engine = $engine;
            fn to_engine(self) -> $engine {
                $engine { $( $field: crate::values::ToEngine::to_engine(self.$field) ),* }
            }
        }
        mirror_record!(@forward $name <=> $engine { $( $field ),* });
    };
    (
        forward_only
        $(#[$meta:meta])*
        $name:ident <=> $engine:path {
            $( $(#[$fmeta:meta])* $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        mirror_record!(@type $(#[$meta])* $name { $( $(#[$fmeta])* $field : $ty ),* });
        mirror_record!(@forward $name <=> $engine { $( $field ),* });
    };
    (@type $(#[$meta:meta])* $name:ident { $( $(#[$fmeta:meta])* $field:ident : $ty:ty ),* }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, uniffi::Record)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty ),*
        }
    };
    (@forward $name:ident <=> $engine:path { $( $field:ident ),* }) => {
        impl crate::values::ToMirror for $engine {
            type Mirror = $name;
            fn to_mirror(self) -> $name {
                $name { $( $field: crate::values::ToMirror::to_mirror(self.$field) ),* }
            }
        }
    };
}

/// An enum mirror whose cases carry nothing.
macro_rules! mirror_unit_enum {
    (
        $(#[$meta:meta])*
        $name:ident <=> $engine:path { $( $(#[$vmeta:meta])* $variant:ident ),* $(,)? }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, uniffi::Enum)]
        pub enum $name {
            $( $(#[$vmeta])* $variant ),*
        }
        impl crate::values::ToMirror for $engine {
            type Mirror = $name;
            fn to_mirror(self) -> $name {
                match self { $( <$engine>::$variant => $name::$variant ),* }
            }
        }
        impl crate::values::ToEngine for $name {
            type Engine = $engine;
            fn to_engine(self) -> $engine {
                match self { $( $name::$variant => <$engine>::$variant ),* }
            }
        }
    };
}

pub(crate) use {mirror_record, mirror_unit_enum};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_and_maps_cross_as_hash_collections_and_come_back_ordered() {
        let set: BTreeSet<String> = ["b", "a"].iter().map(|s| s.to_string()).collect();
        let mirrored = set.clone().to_mirror();
        assert_eq!(mirrored.len(), 2);
        assert_eq!(mirrored.to_engine(), set);
        let map: BTreeMap<String, Vec<i64>> = BTreeMap::from([("k".to_string(), vec![1, 2])]);
        assert_eq!(map.clone().to_mirror().to_engine(), map);
    }
}
