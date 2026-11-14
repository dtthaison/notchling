macro_rules! kinds {
    (@retired) => {
        false
    };
    (@retired $retired:expr) => {
        $retired
    };
    (
        group: $group:expr,
        $(
            $name:ident = $kind:literal, kv $kv:literal {
                title: $title:literal,
                records: $records:literal,
                why: $why:literal,
                default: $default:ident,
                lock: $lock:expr,
                sinks: $($sink:ident)|+,
                durability: $durability:expr,
                level: $level:expr,
                emitters: $emitters:expr,
                agents_read: $agents:expr,
                keep_days: $keep:literal,
                $(retired: $retired:expr,)?
                fields { $($field:ident : $ty:ty),* $(,)? }
            }
        )+
    ) => {
        $(
            #[derive(Clone, Debug)]
            pub struct $name {
                $(pub $field: $ty,)*
            }

            impl $crate::event::Kind for $name {
                const SPEC: &'static $crate::spec::KindSpec = &$crate::spec::KindSpec {
                    name: $kind,
                    kv: $kv,
                    group: $group,
                    title: $title,
                    records: $records,
                    why: $why,
                    default: $crate::spec::DefaultState::$default,
                    lock: $lock,
                    sinks: $crate::spec::Sinks::NONE $(.with($crate::spec::Sinks::$sink))+,
                    durability: $durability,
                    level: $level,
                    emitters: $emitters,
                    agents_read: $agents,
                    keep_days: $keep,
                    retired: kinds!(@retired $($retired)?),
                    fields: &[
                        $(
                            $crate::spec::FieldSpec {
                                name: stringify!($field),
                                desc: <$ty as $crate::field::LogField>::DESC,
                            },
                        )*
                    ],
                };

                fn fields(&self) -> $crate::value::Fields {
                    $crate::value::Fields::new(vec![
                        $((
                            stringify!($field),
                            <$ty as $crate::field::LogField>::value(&self.$field),
                        ),)*
                    ])
                }
            }
        )+

        pub const KINDS: &[&'static $crate::spec::KindSpec] = &[
            $(<$name as $crate::event::Kind>::SPEC,)+
        ];
    };
}
