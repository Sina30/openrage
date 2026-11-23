/// The text in the language in use; the English text is the key, so with no catalog for the language it is the text itself.
///
/// With `name = value` arguments each `{name}` of the text is replaced, and the text and its arguments must agree for the build to pass.
macro_rules! tr {
    ($text:literal) => {
        $crate::i18n::tr($text)
    };
    ($text:literal, $($name:ident = $value:expr),+ $(,)?) => {
        // Each value is evaluated once, whatever it moves, and then named as the text names it.
        match ($(&$value,)+) {
            ($($name,)+) => {
                let _ = || ::std::format!($text, $($name = $name),+);
                $crate::i18n::render(
                    $crate::i18n::tr($text),
                    &[$((::std::stringify!($name), $name as &dyn ::std::fmt::Display)),+],
                )
            }
        }
    };
}

/// The text of the plural entry `one`/`other` for the number `count`, which `{count}` stands for in a form that names it.
macro_rules! tr_n {
    ($one:literal, $other:literal, $count:expr $(, $name:ident = $value:expr)* $(,)?) => {
        match ($count, $(&$value,)*) {
            (count, $($name,)*) => {
                // A form that does not say the number still agrees with its arguments: the number is appended to the check only.
                let _ = || {
                    (
                        ::std::format!(::std::concat!($one, "{count}"), count = count $(, $name = $name)*),
                        ::std::format!(::std::concat!($other, "{count}"), count = count $(, $name = $name)*),
                    )
                };
                $crate::i18n::render(
                    $crate::i18n::tr_n($one, $other, count as u64),
                    &[
                        ("count", &count as &dyn ::std::fmt::Display)
                        $(, (::std::stringify!($name), $name as &dyn ::std::fmt::Display))*
                    ],
                )
            }
        }
    };
}

/// An English text that is shown later, as a constant or a table holds it: the text itself, which `i18n::tr` turns into the language in use where it is drawn.
macro_rules! tr_key {
    ($text:literal) => {
        $text
    };
}
