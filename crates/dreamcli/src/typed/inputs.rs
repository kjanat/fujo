use crate::Resolved;
use crate::Result;
use crate::schema::ArgMarker;
use crate::schema::FlagMarker;
use crate::schema::Input;
use crate::schema::InputSchema;
use crate::schema::Presence;

mod sealed {
    pub trait Sealed {}
}

/// Registered inputs and the native references delivered to a typed callback.
/// Implemented for individual inputs, `()`, and tuples of up to twelve sets.
/// Sealed so a custom reader cannot silently access undeclared inputs.
pub trait InputSet: sealed::Sealed + Send + Sync + 'static {
    type Values<'a>;
    #[doc(hidden)]
    fn visit(&self, visitor: &mut dyn FnMut(InputSchema));
    #[doc(hidden)]
    fn read<'a>(&self, resolved: &'a Resolved) -> Result<Self::Values<'a>>;
}

/// A set containing only flags, suitable for CLI-wide registration.
pub trait FlagSet: InputSet {}

impl sealed::Sealed for () {}
impl InputSet for () {
    type Values<'a> = ();
    fn visit(&self, _: &mut dyn FnMut(InputSchema)) {}
    fn read<'a>(&self, _: &'a Resolved) -> Result<Self::Values<'a>> {
        Ok(())
    }
}
impl FlagSet for () {}

macro_rules! input {
    ($surface:ty) => {
        impl<T: 'static, P: Presence<T> + 'static> sealed::Sealed for Input<T, P, $surface> {}
        impl<T: 'static, P: Presence<T> + 'static> InputSet for Input<T, P, $surface> {
            type Values<'a> = P::Ref<'a>;
            fn visit(&self, visitor: &mut dyn FnMut(InputSchema)) {
                visitor(self.schema().clone());
            }
            fn read<'a>(&self, resolved: &'a Resolved) -> Result<Self::Values<'a>> {
                resolved.get_ref(self)
            }
        }
    };
}
input!(FlagMarker);
input!(ArgMarker);
impl<T: 'static, P: Presence<T> + 'static> FlagSet for Input<T, P, FlagMarker> {}

macro_rules! tuple {
    ($($ty:ident:$index:tt),+) => {
        impl<$($ty: InputSet),+> sealed::Sealed for ($($ty,)+) {}
        impl<$($ty: InputSet),+> InputSet for ($($ty,)+) {
            type Values<'a> = ($($ty::Values<'a>,)+);
            fn visit(&self, visitor: &mut dyn FnMut(InputSchema)) { $(self.$index.visit(visitor);)+ }
            fn read<'a>(&self, resolved: &'a Resolved) -> Result<Self::Values<'a>> {
                Ok(($(self.$index.read(resolved)?,)+))
            }
        }
        impl<$($ty: FlagSet),+> FlagSet for ($($ty,)+) {}
    };
}
tuple!(A:0);
tuple!(A:0,B:1);
tuple!(A:0,B:1,C:2);
tuple!(A:0,B:1,C:2,D:3);
tuple!(A:0,B:1,C:2,D:3,E:4);
tuple!(A:0,B:1,C:2,D:3,E:4,F:5);
tuple!(A:0,B:1,C:2,D:3,E:4,F:5,G:6);
tuple!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7);
tuple!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8);
tuple!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9);
tuple!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9,K:10);
tuple!(A:0,B:1,C:2,D:3,E:4,F:5,G:6,H:7,I:8,J:9,K:10,L:11);
