//! Data-driven HAL pin catalog generation for userspace components.

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_pin_field_type {
    (bit) => { *mut $crate::hal_bit_t };
    (s32) => { *mut $crate::hal_s32_t };
    (u32) => { *mut $crate::hal_u32_t };
    (float) => { *mut $crate::real_t };
    (bit[$length:expr]) => { [*mut $crate::hal_bit_t; $length] };
    (s32[$length:expr]) => { [*mut $crate::hal_s32_t; $length] };
    (u32[$length:expr]) => { [*mut $crate::hal_u32_t; $length] };
    (float[$length:expr]) => { [*mut $crate::real_t; $length] };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_empty_pin_field {
    ($kind:ident) => {
        ::core::ptr::null_mut()
    };
    ($kind:ident[$length:expr]) => {
        [::core::ptr::null_mut(); $length]
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_pin_field_count {
    ($kind:ident) => {
        1usize
    };
    ($kind:ident[$length:expr]) => {
        $length
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_pin_kind {
    (bit) => {
        $crate::HalPinKind::Bit
    };
    (s32) => {
        $crate::HalPinKind::S32
    };
    (u32) => {
        $crate::HalPinKind::U32
    };
    (float) => {
        $crate::HalPinKind::Float
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_pin_zero_value {
    (bit) => {
        false
    };
    (s32) => {
        0i32
    };
    (u32) => {
        0u32
    };
    (float) => {
        0.0f64
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_pin_direction {
    (in) => {
        $crate::HalPinDirection::In
    };
    (out) => {
        $crate::HalPinDirection::Out
    };
    (io) => {
        $crate::HalPinDirection::Io
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_register_catalog_field {
    ($register:path, $pins:ident, $component:ident, $component_id:ident,
     $field:ident: $kind:ident $direction:ident => $suffix:expr) => {{
        let suffix: &str = $suffix;
        unsafe {
            $register(
                $component,
                suffix,
                &mut $pins.$field,
                $component_id,
                $crate::__hal_pin_direction!($direction),
            )?;
        }
    }};
    ($register:path, $pins:ident, $component:ident, $component_id:ident,
     $field:ident: $kind:ident[$length:expr] $direction:ident => $suffixes:expr) => {{
        for (index, suffix) in ($suffixes).into_iter().enumerate() {
            let suffix: &str = suffix.as_ref();
            unsafe {
                $register(
                    $component,
                    suffix,
                    &mut $pins.$field[index],
                    $component_id,
                    $crate::__hal_pin_direction!($direction),
                )?;
            }
        }
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_describe_catalog_field {
    ($schema:ident, $field:ident: $kind:ident $direction:ident => $suffix:expr) => {
        $schema.push((
            ($suffix).to_owned(),
            $crate::__hal_pin_kind!($kind),
            $crate::__hal_pin_direction!($direction),
        ));
    };
    ($schema:ident, $field:ident: $kind:ident[$length:expr] $direction:ident => $suffixes:expr) => {
        $schema.extend(($suffixes).into_iter().map(|suffix| {
            let suffix: &str = suffix.as_ref();
            (
                suffix.to_owned(),
                $crate::__hal_pin_kind!($kind),
                $crate::__hal_pin_direction!($direction),
            )
        }));
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __hal_zero_catalog_field {
    ($pins:ident, $field:ident: $kind:ident) => {
        unsafe {
            ::core::ptr::write_volatile($pins.$field, $crate::__hal_pin_zero_value!($kind));
        }
    };
    ($pins:ident, $field:ident: $kind:ident[$length:expr]) => {
        for pointer in $pins.$field {
            unsafe {
                ::core::ptr::write_volatile(pointer, $crate::__hal_pin_zero_value!($kind));
            }
        }
    };
}

/// Declare every property of a userspace component's HAL pins once.
///
/// The catalog generates the pointer storage, null initialization, typed
/// registration, zero initialization, exact pin count, and the schema consumed
/// by tests. Array name expressions may return either `&str` or `String`
/// values.
#[macro_export]
macro_rules! userspace_hal_pin_catalog {
    (
        $visibility:vis struct $name:ident;
        error = $error:ty;
        register = $register:path;
        pins {
            $($field:ident: $kind:ident $([$length:expr])? $direction:ident => $suffixes:expr;)+
        }
        groups { $($group_field:ident: $group_type:ty;)* }
    ) => {
        $visibility struct $name {
            $(pub(super) $field: $crate::__hal_pin_field_type!($kind $([$length])?),)+
            $(pub(super) $group_field: $group_type,)*
        }

        impl $name {
            pub(super) const PIN_COUNT: usize =
                0 $(+ $crate::__hal_pin_field_count!($kind $([$length])?))+
                $(+ <$group_type>::PIN_COUNT)*;

            pub(super) const fn empty() -> Self {
                Self {
                    $($field: $crate::__hal_empty_pin_field!($kind $([$length])?),)+
                    $($group_field: <$group_type>::empty(),)*
                }
            }

            pub(super) unsafe fn register(
                &mut self,
                component: &str,
                component_id: ::core::ffi::c_int,
            ) -> Result<(), $error> {
                $($crate::__hal_register_catalog_field!(
                    $register,
                    self,
                    component,
                    component_id,
                    $field: $kind $([$length])? $direction => $suffixes
                );)+
                $(unsafe {
                    self.$group_field.register(component, component_id)?;
                })*
                Ok(())
            }

            /// Initialize every registered pin to the zero value for its
            /// LinuxCNC HAL type. This is generated from the same catalog as
            /// registration, so adding a pin cannot omit its initialization.
            pub(super) unsafe fn initialize_zero(&self) {
                $($crate::__hal_zero_catalog_field!(
                    self,
                    $field: $kind $([$length])?
                );)+
                $(unsafe { self.$group_field.initialize_zero(); })*
            }

            #[cfg(test)]
            pub(super) fn schema() -> ::std::vec::Vec<(
                ::std::string::String,
                $crate::HalPinKind,
                $crate::HalPinDirection,
            )> {
                let mut schema = ::std::vec::Vec::with_capacity(Self::PIN_COUNT);
                $($crate::__hal_describe_catalog_field!(
                    schema,
                    $field: $kind $([$length])? $direction => $suffixes
                );)+
                $(schema.extend(<$group_type>::schema());)*
                schema
            }
        }
    };
}

/// Output-only shorthand for telemetry publishers.
#[macro_export]
macro_rules! userspace_hal_output_catalog {
    (
        $visibility:vis struct $name:ident;
        error = $error:ty;
        register = $register:path;
        $($field:ident: $kind:ident $([$length:expr])? => $suffixes:expr;)+
    ) => {
        $crate::userspace_hal_pin_catalog! {
            $visibility struct $name;
            error = $error;
            register = $register;
            pins {
                $($field: $kind $([$length])? out => $suffixes;)+
            }
            groups {}
        }
    };
}

/// Generate the common LinuxCNC userspace HAL component lifecycle once.
///
/// Component and pin names are checked against the audited LinuxCNC limit,
/// every call is classified by its exact API identity, partial registration is
/// cleaned up exactly once, and the component-specific initializer runs before
/// `hal_ready` exposes the pins.
#[macro_export]
macro_rules! userspace_hal_component {
    (
        error $error_visibility:vis $error:ident;
        register $register_visibility:vis $register:ident;
        create $create_visibility:vis $create:ident;
        pins $pins:ty;
        initialize $initialize:path;
    ) => {
        $error_visibility type $error =
            $crate::HalRegistrationError<::std::string::String>;

        $register_visibility unsafe fn $register<T: $crate::HalPinValue>(
            component: &str,
            suffix: &str,
            pointer: *mut *mut T,
            component_id: ::core::ffi::c_int,
            direction: $crate::HalPinDirection,
        ) -> Result<(), $error> {
            let name = ::std::format!("{component}.{suffix}");
            if name.len() > $crate::HAL_NAME_LEN as usize {
                return Err($crate::HalRegistrationError::new(
                    $crate::HalRegistrationFailure::NameTooLong {
                        kind: $crate::HalNameKind::Pin,
                        context: suffix.to_owned(),
                        length: name.len(),
                        maximum: $crate::HAL_NAME_LEN as usize,
                    },
                ));
            }
            let name = ::std::ffi::CString::new(name).map_err(|error| {
                $crate::HalRegistrationError::new(
                    $crate::HalRegistrationFailure::InvalidCString {
                        kind: $crate::HalNameKind::Pin,
                        context: suffix.to_owned(),
                        nul_position: error.nul_position(),
                    },
                )
            })?;
            unsafe { $crate::register_pin(name.as_ptr(), direction, pointer, component_id) }
                .map_err(|error| {
                    $crate::HalRegistrationError::new(
                        $crate::HalRegistrationFailure::Call {
                            error,
                            context: Some(suffix.to_owned()),
                        },
                    )
                })
        }

        $create_visibility unsafe fn $create(
            component: &str,
        ) -> Result<(::core::ffi::c_int, *mut $pins), $error> {
            if component.len() > $crate::HAL_NAME_LEN as usize {
                return Err($crate::HalRegistrationError::new(
                    $crate::HalRegistrationFailure::NameTooLong {
                        kind: $crate::HalNameKind::Component,
                        context: component.to_owned(),
                        length: component.len(),
                        maximum: $crate::HAL_NAME_LEN as usize,
                    },
                ));
            }
            let component_name = ::std::ffi::CString::new(component).map_err(|error| {
                $crate::HalRegistrationError::new(
                    $crate::HalRegistrationFailure::InvalidCString {
                        kind: $crate::HalNameKind::Component,
                        context: component.to_owned(),
                        nul_position: error.nul_position(),
                    },
                )
            })?;
            let component_id = $crate::HalCall::Init
                .classify(unsafe { $crate::hal_init(component_name.as_ptr()) })
                .map_err(|error| {
                    $crate::HalRegistrationError::new(
                        $crate::HalRegistrationFailure::Call {
                            error,
                            context: None,
                        },
                    )
                })?;

            let result = (|| {
                let bytes = ::core::mem::size_of::<$pins>();
                let pins = unsafe { $crate::hal_malloc(bytes as _) } as *mut $pins;
                if pins.is_null() {
                    return Err($crate::HalRegistrationError::new(
                        $crate::HalRegistrationFailure::Allocation { bytes },
                    ));
                }
                unsafe {
                    ::core::ptr::write(pins, <$pins>::empty());
                    (&mut *pins).register(component, component_id)?;
                    $initialize(&*pins);
                }
                $crate::HalCall::Ready
                    .classify(unsafe { $crate::hal_ready(component_id) })
                    .map_err(|error| {
                        $crate::HalRegistrationError::new(
                            $crate::HalRegistrationFailure::Call {
                                error,
                                context: None,
                            },
                        )
                    })?;
                Ok((component_id, pins))
            })();

            match result {
                Ok(value) => Ok(value),
                Err(error) => match $crate::HalCall::Exit
                    .classify(unsafe { $crate::hal_exit(component_id) })
                {
                    Ok(_) => Err(error),
                    Err(cleanup) => Err(error.with_cleanup(cleanup)),
                },
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __realtime_register_catalog_field {
    ($pins:ident, $component_id:ident,
     $field:ident: $kind:ident $direction:ident => $name:literal) => {{
        const _: () = assert!($name.len() <= $crate::HAL_NAME_LEN as usize);
        unsafe {
            $crate::register_pin(
                ::core::concat!($name, "\0").as_ptr().cast(),
                $crate::__hal_pin_direction!($direction),
                ::core::ptr::addr_of_mut!((*$pins).$field),
                $component_id,
            )?;
        }
    }};
    ($pins:ident, $component_id:ident,
     $field:ident: $kind:ident[$length:expr] $direction:ident => [$($name:literal),+ $(,)?]) => {{
        $(const _: () = assert!($name.len() <= $crate::HAL_NAME_LEN as usize);)+
        const NAMES: [&[u8]; $length] = [$(::core::concat!($name, "\0").as_bytes()),+];
        for (index, name) in NAMES.iter().copied().enumerate() {
            unsafe {
                $crate::register_pin(
                    name.as_ptr().cast(),
                    $crate::__hal_pin_direction!($direction),
                    ::core::ptr::addr_of_mut!((*$pins).$field[index]),
                    $component_id,
                )?;
            }
        }
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __realtime_register_numbered_catalog_field {
    ($pins:ident, $component_id:ident,
     $field:ident: $kind:ident[$length:expr] $direction:ident =>
     ($prefix:literal, $values:expr)) => {{
        const _: () = assert!($prefix.len() < $crate::HAL_NAME_LEN as usize);
        for (index, value) in ($values).iter().copied().enumerate() {
            unsafe {
                $crate::register_numbered_pin(
                    $prefix.as_bytes(),
                    value.wire_code() as u32,
                    $crate::__hal_pin_direction!($direction),
                    ::core::ptr::addr_of_mut!((*$pins).$field[index]),
                    $component_id,
                )?;
            }
        }
    }};
}

/// Declare and register a no-allocation realtime HAL pin schema from one table.
///
/// Fixed names are string literals. `numbered` entries pair a prefix with a
/// diagnostic catalog whose values provide `wire_code()`. The generated struct,
/// exact count, pin types, directions, and registration sequence therefore
/// cannot drift into separate handwritten definitions.
#[macro_export]
macro_rules! realtime_hal_pin_catalog {
    (
        $struct_visibility:vis struct $name:ident;
        $register_visibility:vis fn $register:ident;
        pins {
            $($field:ident: $kind:ident $([$length:expr])? $direction:ident => $names:tt;)+
        }
        numbered {
            $($numbered_field:ident: $numbered_kind:ident[$numbered_length:expr]
              $numbered_direction:ident => $numbered_names:tt;)*
        }
    ) => {
        $struct_visibility struct $name {
            $(pub(super) $field: $crate::__hal_pin_field_type!($kind $([$length])?),)+
            $(pub(super) $numbered_field:
                $crate::__hal_pin_field_type!($numbered_kind[$numbered_length]),)*
        }

        impl $name {
            pub(crate) const PIN_COUNT: usize =
                0 $(+ $crate::__hal_pin_field_count!($kind $([$length])?))+
                $(+ $crate::__hal_pin_field_count!(
                    $numbered_kind[$numbered_length]
                ))*;
        }

        $register_visibility unsafe fn $register(
            pins: *mut $name,
            component_id: ::core::ffi::c_int,
        ) -> Result<(), $crate::HalError> {
            $($crate::__realtime_register_catalog_field!(
                pins,
                component_id,
                $field: $kind $([$length])? $direction => $names
            );)+
            $($crate::__realtime_register_numbered_catalog_field!(
                pins,
                component_id,
                $numbered_field: $numbered_kind[$numbered_length]
                    $numbered_direction => $numbered_names
            );)*
            Ok(())
        }
    };
}
