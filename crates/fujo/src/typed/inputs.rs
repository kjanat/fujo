use crate::Resolved;
use crate::Result;
use crate::schema::ArgMarker;
use crate::schema::FlagMarker;
use crate::schema::Input;
use crate::schema::InputSchema;
use crate::schema::Presence;

pub(crate) mod sealed {
    use super::InputSchema;
    use super::InputSet;
    use super::Resolved;
    use super::Result;

    pub trait Sealed {}

    pub trait Shape: InputSet {
        type Refs<'s>;
        fn visit_refs(refs: &Self::Refs<'_>, visitor: &mut dyn FnMut(InputSchema));
        fn read_refs<'a>(refs: &Self::Refs<'_>, resolved: &'a Resolved)
        -> Result<Self::Values<'a>>;
    }
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

/// The inputs of a pipeline `I` after it inherits a pipeline over `G`. Stages
/// read the values of `I`; the set counts as flags only when `G` does too.
pub struct Inherits<I, G> {
    pub(crate) inputs: std::sync::Arc<I>,
    pub(crate) inherited: std::marker::PhantomData<fn() -> G>,
}
impl<I: InputSet, G: InputSet> sealed::Sealed for Inherits<I, G> {}
impl<I: InputSet, G: InputSet> InputSet for Inherits<I, G> {
    type Values<'a> = I::Values<'a>;
    fn visit(&self, visitor: &mut dyn FnMut(InputSchema)) {
        self.inputs.visit(visitor);
    }
    fn read<'a>(&self, resolved: &'a Resolved) -> Result<Self::Values<'a>> {
        self.inputs.read(resolved)
    }
}
impl<I: FlagSet, G: FlagSet> FlagSet for Inherits<I, G> {}

/// A named input struct declared with [`inputs!`](crate::inputs) or
/// [`flags!`](crate::flags). The struct exposes its fields as a tuple of
/// handles, and every value it delivers is read through those handles.
pub trait Bundle: Send + Sync + 'static {
    #[doc(hidden)]
    type Shape: sealed::Shape;
    type Values<'a>;
    #[doc(hidden)]
    fn fields(&self) -> <Self::Shape as sealed::Shape>::Refs<'_>;
    #[doc(hidden)]
    fn values<'a>(fields: <Self::Shape as InputSet>::Values<'a>) -> Self::Values<'a>;
}
impl<B: Bundle> sealed::Sealed for B {}
impl<B: Bundle> InputSet for B {
    type Values<'a> = B::Values<'a>;
    fn visit(&self, visitor: &mut dyn FnMut(InputSchema)) {
        <B::Shape as sealed::Shape>::visit_refs(&self.fields(), visitor);
    }
    fn read<'a>(&self, resolved: &'a Resolved) -> Result<Self::Values<'a>> {
        Ok(B::values(<B::Shape as sealed::Shape>::read_refs(
            &self.fields(),
            resolved,
        )?))
    }
}
impl<B: Bundle> FlagSet for B where B::Shape: FlagSet {}

macro_rules! input {
    ($surface:ty) => {
        impl<T: 'static, P: Presence<T> + 'static> sealed::Sealed for Input<T, P, $surface> {}
        impl<T: 'static, P: Presence<T> + 'static> InputSet for Input<T, P, $surface> {
            type Values<'a> = P::Ref<'a>;
            fn visit(&self, visitor: &mut dyn FnMut(InputSchema)) {
                visitor(self.typed_schema());
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
        impl<$($ty: InputSet),+> sealed::Shape for ($($ty,)+) {
            type Refs<'s> = ($(&'s $ty,)+);
            fn visit_refs(refs: &Self::Refs<'_>, visitor: &mut dyn FnMut(InputSchema)) {
                $(refs.$index.visit(visitor);)+
            }
            fn read_refs<'a>(refs: &Self::Refs<'_>, resolved: &'a Resolved)
            -> Result<Self::Values<'a>> {
                Ok(($(refs.$index.read(resolved)?,)+))
            }
        }
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

/// Declare a named input bundle. The first struct holds input handles and is
/// `Clone`; the second receives their resolved references in typed callbacks.
/// Each field is any [`InputSet`]: an input handle, a tuple, or another
/// declared bundle.
///
/// ```
/// use fujo::{Arg, Cli, Command, Flag, Required, arg, flag};
/// use std::path::PathBuf;
/// fujo::inputs! {
///     struct Build => BuildValues {
///         root: Arg<PathBuf>,
///         jobs: Flag<u16, Required>,
///         label: Flag<String>,
///     }
/// }
/// let command = Command::new("build")
///     .inputs(Build {
///         root: arg::path("root"),
///         jobs: flag::parse("jobs").default(4),
///         label: flag::string("label"),
///     })
///     .action(|ctx, BuildValues { root, jobs, label }, ()| {
///         let label = label.map_or("build", String::as_str);
///         ctx.out.log(format!("{label}: {} with {jobs} jobs", root.display()));
///         Ok(())
///     });
/// let result = Cli::new("build").default(command).execute(["src"], Default::default());
/// assert_eq!(result.stdout, "build: src with 4 jobs\n");
/// ```
///
/// Field types are checked against the handles supplied at construction:
/// ```compile_fail
/// use fujo::{Arg, flag};
/// fujo::inputs! { struct Demo => DemoValues { name: Arg<String> } }
/// let demo = Demo { name: flag::string("name") };
/// ```
///
/// Only declared bundles and the built-in sets implement `InputSet`:
/// ```compile_fail
/// struct Sneaky;
/// impl fujo::InputSet for Sneaky {
///     type Values<'a> = ();
///     fn visit(&self, _: &mut dyn FnMut(fujo::schema::InputSchema)) {}
///     fn read<'a>(&self, _: &'a fujo::Resolved) -> fujo::Result<()> {
///         Ok(())
///     }
/// }
/// ```
#[macro_export]
macro_rules! inputs {
    (
        $(#[$meta:meta])* $vis:vis struct $name:ident => $values:ident {
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident : $ty:ty),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $($(#[$field_meta])* $field_vis $field: $ty,)+
        }
        impl Clone for $name {
            fn clone(&self) -> Self {
                Self { $($field: self.$field.clone(),)+ }
            }
        }
        #[derive(Clone, Copy)]
        $vis struct $values<'a> {
            $($field_vis $field: <$ty as $crate::InputSet>::Values<'a>,)+
        }
        impl $crate::Bundle for $name {
            type Shape = ($($ty,)+);
            type Values<'a> = $values<'a>;
            fn fields(&self) -> ($(&$ty,)+) {
                ($(&self.$field,)+)
            }
            fn values<'a>(
                ($($field,)+): ($(<$ty as $crate::InputSet>::Values<'a>,)+),
            ) -> $values<'a> {
                $values { $($field,)+ }
            }
        }
    };
}

/// Declare a named bundle containing only flags, accepted by `Cli::inputs` as
/// shared globals and by `Command::inputs` as a nested field.
///
/// ```
/// use fujo::{Cli, Command, Flag, Required, flag};
/// fujo::flags! {
///     struct Globals => GlobalValues {
///         profile: Flag<String, Required>,
///     }
/// }
/// let globals = Globals { profile: flag::string("profile").default("dev".into()) };
/// let command = Command::new("show")
///     .inputs(globals.clone())
///     .action(|ctx, GlobalValues { profile }, ()| {
///         ctx.out.log(profile);
///         Ok(())
///     });
/// let app = Cli::new("demo").inputs(globals).command(command);
/// assert_eq!(app.execute(["--profile=prod", "show"], Default::default()).stdout, "prod\n");
/// ```
///
/// A positional argument is rejected:
/// ```compile_fail
/// use fujo::Arg;
/// fujo::flags! { struct Globals => GlobalValues { name: Arg<String> } }
/// ```
#[macro_export]
macro_rules! flags {
    (
        $(#[$meta:meta])* $vis:vis struct $name:ident => $values:ident {
            $($(#[$field_meta:meta])* $field_vis:vis $field:ident : $ty:ty),+ $(,)?
        }
    ) => {
        $crate::inputs! {
            $(#[$meta])* $vis struct $name => $values {
                $($(#[$field_meta])* $field_vis $field: $ty,)+
            }
        }
        const _: fn() = || {
            fn flags<T: $crate::typed::FlagSet>() {}
            flags::<($($ty,)+)>();
        };
    };
}
