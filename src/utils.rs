#[macro_export]
macro_rules! erase_trait {
    (
        $vtable:ident, $erased:ident, $trait:ident {
            $(fn $method:ident(&self $(, $arg:ident: $ty:ty)*) -> $ret:ty;)*
        }
    ) => {
        pub struct $vtable
        {
            drop: unsafe fn(*mut ()),
            $($method: unsafe fn(*const () $(, $ty)*) -> $ret,)*
        }

        impl $vtable
        {
            pub const fn for_type<T: $trait>() -> Self
            {
                unsafe fn drop_fn<T>(ptr: *mut ())
                {
                    drop(unsafe { Box::from_raw(ptr as *mut T) });
                }
                $(
                    unsafe fn $method<T: $trait>(ptr: *const () $(, $arg: $ty)*) -> $ret
                    {
                        (unsafe { &*(ptr as *const T) }).$method($($arg),*)
                    }
                )*

                Self {
                    drop: drop_fn::<T>,
                    $($method: $method::<T>,)*
                }
            }
        }

        pub struct $erased
        {
            data:   *mut (),
            vtable: &'static $vtable,
        }

        impl $erased
        {
            pub fn new<T: $trait>(value: T) -> Self
            {
                Self {
                    data:   Box::into_raw(Box::new(value)) as *mut (),
                    vtable: Box::leak(Box::new($vtable::for_type::<T>())),
                }
            }

            $(
                pub fn $method(&self $(, $arg: $ty)*) -> $ret
                {
                    unsafe { (self.vtable.$method)(self.data $(, $arg)*) }
                }
            )*
        }

        impl Drop for $erased
        {
            fn drop(&mut self)
            {
                unsafe { (self.vtable.drop)(self.data) };
            }
        }
    };
}
