#![feature(try_trait, alloc_layout_extra)]
#![forbid(missing_docs)]

/*!
# vec-utils

This is an experimental crate that adds some helpful functionality to `Vec<T>`, like `map` and `zip_with`. These functions allow you to transform a vec and try and reuse the allocation if possible!

```rust
use vec_utils::VecExt;

fn to_bits(v: Vec<f32>) -> Vec<u32> {
    v.map(|x| x.to_bits())
}

fn sum_2(v: Vec<f32>, w: Vec<f64>) -> Vec<f64> {
    v.zip_with(w, |x, y| f64::from(x) + y)
}
```

But `zip_with` is limited to taking only a single additional vector. To get around this limitation, this crate also exports some macros that can take an arbitrary number of input vectors, and in most cases will compile down to the same assembly as `Vec::map` and `Vec::zip_with` (sometimes with some additional cleanup code, but even then the macro solution is just as fast as the built-in version).

You can use the `zip_with` and `try_zip_with` macros like so,

```rust
use vec_utils::{zip_with, try_zip_with};

fn to_bits(v: Vec<f32>) -> Vec<u32> {
    zip_with!(v, |x| x.to_bits())
}

fn sum_2(v: Vec<f32>, w: Vec<f64>) -> Vec<f64> {
    zip_with!((v, w), |x, y| f64::from(x) + y)
}

fn sum_5(a: Vec<i32>, b: Vec<i32>, c: Vec<i32>, d: Vec<i32>, e: Vec<i32>) -> Vec<i32> {
    zip_with!((a, b, c, d, e), |a, b, c, d, e| a + b + c + d + e)
}

fn mul_with(a: Vec<i32>) -> Vec<i32> {
    zip_with!((a, vec![0, 1, 2, 3, 4, 5, 6, 7]), |a, x| a * x)
}

fn to_bits_no_nans(v: Vec<f32>) -> Result<Vec<u32>, &'static str> {
    try_zip_with!(v, |x| if x.is_nan() { Err("Found NaN!") } else { Ok(x.to_bits()) })
}
```

You can use as many input vectors as you want, just put them all inside the input tuple. Note that the second argument is not a closure, but syntax that looks like a closure, i.e. you can't make a closure before-hand and pass it as the second argument. Also, you can't use general patterns in the "closure"'s arguments, only identifiers are allowed. You can specify if you want a move closure by adding the move keyword in from of the "closure".

```rust
use vec_utils::zip_with;

fn add(a: Vec<i32>, b: i32) -> Vec<i32> {
    zip_with!(a, move |a| a + b)
}
```

It also adds some functionality to reuse the allocation of a `Box<T>`, using the `BoxExt`/`UninitBox` api.

```rust
use vec_utils::BoxExt;

fn replace(b: Box<i32>, f: f32) -> Box<f32> {
    Box::drop_box(b).init(f)
}

fn get_and_replace(b: Box<i32>, f: f32) -> (Box<f32>, i32) {
    let (b, x) = Box::take_box(b);
    (b.init(f), x)
}
```
*/

/// This allows running destructors, even if other destructors have panicked
macro_rules! defer {
    ($($do_work:tt)*) => {
        let _guard = $crate::OnDrop(Some(|| { $($do_work)* }));
    }
}

/// A macro to give syntactic sugar for `general_zip::try_zip_with`
///
/// This allows combining multiple vectors into a one with short-circuiting
/// on the failure case
///
/// # Usage
///
/// ```rust
/// use vec_utils::{zip_with, try_zip_with};
///
/// fn to_bits(v: Vec<f32>) -> Vec<u32> {
///    zip_with!(v, |x| x.to_bits())
/// }
///
/// fn sum_2(v: Vec<f32>, w: Vec<f64>) -> Vec<f64> {
///    zip_with!((v, w), |x, y| f64::from(x) + y)
/// }
///
/// fn sum_5(a: Vec<i32>, b: Vec<i32>, c: Vec<i32>, d: Vec<i32>, e: Vec<i32>) -> Vec<i32> {
///    zip_with!((a, b, c, d, e), |a, b, c, d, e| a + b + c + d + e)
/// }
///
/// fn mul_with(a: Vec<i32>) -> Vec<i32> {
///    zip_with!((a, vec![0, 1, 2, 3, 4, 5, 6, 7]), |a, x| a * x)
/// }
///
/// fn to_bits_no_nans(v: Vec<f32>) -> Result<Vec<u32>, &'static str> {
///     try_zip_with!(v, |x| if x.is_nan() { Err("Found NaN!") } else { Ok(x.to_bits()) })
/// }
/// ```
/// You can use as many input vectors as you want, just put them all inside the input tuple.
/// Note that the second argument is not a closure, but syntax that looks like a closure,
/// i.e. you can't make a closure before-hand and pass it as the second argument. Also, you can't
/// use general patterns in the "closure"'s arguments, only identifiers are allowed.
/// You can specify if you want a move closure by adding the move keyword in from of the "closure".
///
/// ```rust
/// use vec_utils::zip_with;
///
/// fn add(a: Vec<i32>, b: i32) -> Vec<i32> {
///     zip_with!(a, move |a| a + b)
/// }
/// ```
#[macro_export]
macro_rules! try_zip_with {
    ($vec:expr, $($move:ident)? |$($i:ident),+ $(,)?| $($work:tt)*) => {{
        #[allow(unused_parens)]
        let ($($i),*) = $vec;

        $crate::try_zip_with_impl(
            $crate::list!(WRAP $($i),*),
            $($move)? |$crate::list!(PLACE $($i),*)| $($work)*
        )
    }};
}

/// A wrapper around `try_zip_with` for infallible mapping
#[macro_export]
macro_rules! zip_with {
    ($vec:expr, $($move:ident)? |$($i:ident),+ $(,)?| $($work:tt)*) => {
        match $crate::try_zip_with!(
            $vec, $($move)? |$($i),+|
            Ok::<_, std::convert::Infallible>($($work)*)
        ) {
            Ok(x) => x,
            Err(x) => match x {}
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! list {
    (WRAP $e:ident) => {
        ($e,)
    };
    (PLACE $e:ident) => {
        $e
    };
    ($wrap:ident $e:ident $(, $rest:ident)* $(,)?) => {
        ($e, $crate::list!($wrap $($rest),*))
    };
}

struct OnDrop<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        self.0.take().unwrap()()
    }
}

mod boxed;
mod vec;

pub use self::boxed::*;
pub use self::vec::*;
