macro_rules! log_choice {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $($variant:ident = $text:literal),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        $vis enum $name {
            $($variant,)+
            Other,
        }

        impl $name {
            pub const VALUES: &'static [&'static str] = &[$($text,)+ "other"];

            pub fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                    Self::Other => "other",
                }
            }

            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($text => Some(Self::$variant),)+
                    "other" => Some(Self::Other),
                    _ => None,
                }
            }
        }

        impl $crate::field::sealed::Sealed for $name {}

        impl $crate::field::LogField for $name {
            const DESC: $crate::field::FieldDesc = $crate::field::FieldDesc {
                ty: "choice",
                choices: $name::VALUES,
                optional: false,
                list: false,
                ref_kind: None,
                always_hashed: false,
            };

            fn value(&self) -> $crate::field::FieldValue {
                $crate::field::FieldValue::Name(self.name())
            }
        }

        impl $crate::field::Choice for $name {
            fn from_name(name: &str) -> Option<Self> {
                $name::from_name(name)
            }

            fn other() -> Self {
                Self::Other
            }
        }
    };
}
