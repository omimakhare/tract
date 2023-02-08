pub type TractError = anyhow::Error;
pub type TractResult<T> = anyhow::Result<T>;
use std::{fmt};
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum DatumType {
    F32,
}
impl DatumType {
    pub fn is_unsigned(&self) -> bool {
        false
    }
    pub fn is_signed(&self) -> bool {
        false
    }
    pub fn is_float(&self) -> bool {
        matches!(self, DatumType::F32)
    }
    pub fn is_copy(&self) -> bool {
        self.is_unsigned() || self.is_signed() || self.is_float()
    }
    pub fn unquantized(&self) -> DatumType {
        match self {
            _ => *self,
        }
    }
    #[inline]
    pub fn size_of(&self) -> usize {
        std::mem::size_of::<f32>()
    }
    #[inline]
    pub fn alignment(&self) -> usize {
        match self {
            _ => self.size_of(),
        }
    }
}
pub trait Datum:
    Clone + Send + Sync + fmt::Debug + fmt::Display + Default + 'static + PartialEq
{
    fn name() -> &'static str;
    fn datum_type() -> DatumType;
}
macro_rules! datum {
    ($ t : ty , $ v : ident) => {
        impl From<$t> for Tensor {
            fn from(it: $t) -> Tensor {
                tensor0(it)
            }
        }
        impl Datum for $t {
            fn name() -> &'static str {
                stringify!($t)
            }
            fn datum_type() -> DatumType {
                DatumType::$v
            }
        }
    };
}
datum!(f32, F32);
use ndarray::*;
use std::alloc;
pub fn tensor0<A: Datum>(x: A) -> Tensor {
    Tensor::from(arr0(x))
}
#[derive(Eq)]
pub struct Tensor {
    dt: DatumType,
    shape: Vec<usize>,
    strides: Vec<isize>,
    len: usize,
    data: *mut u8,
}
unsafe impl Send for Tensor {}
unsafe impl Sync for Tensor {}
impl Tensor {
    pub unsafe fn uninitialized<T: Datum>(shape: &[usize]) -> anyhow::Result<Tensor> {
        Self::uninitialized_dt(T::datum_type(), shape)
    }
    pub unsafe fn uninitialized_dt(dt: DatumType, shape: &[usize]) -> anyhow::Result<Tensor> {
        Self::uninitialized_aligned_dt(dt, shape, dt.alignment())
    }
    pub unsafe fn uninitialized_aligned<T: Datum>(
        shape: &[usize],
        alignment: usize,
    ) -> anyhow::Result<Tensor> {
        Self::uninitialized_aligned_dt(T::datum_type(), shape, alignment)
    }
    pub unsafe fn uninitialized_aligned_dt(
        dt: DatumType,
        shape: &[usize],
        alignment: usize,
    ) -> anyhow::Result<Tensor> {
        assert!(dt.is_copy());
        let bytes = shape.iter().cloned().product::<usize>() * dt.size_of();
        let layout = alloc::Layout::from_size_align(bytes, alignment)?;
        let data = if bytes == 0 {
            unimplemented!()
        } else {
            let ptr = alloc::alloc(layout);
            assert!(!ptr.is_null());
            ptr
        } as *mut u8;
        let mut tensor = Tensor {
            strides: vec![],
            dt,
            shape: shape.into(),
            data,
            len: 0,
        };
        tensor.update_strides_and_len();
        Ok(tensor)
    }
    pub fn clear<T: Datum + Clone>(&mut self) -> anyhow::Result<()> {
        self.fill_t::<f32>(0.0)
    }
    pub fn zero<T: Datum>(shape: &[usize]) -> anyhow::Result<Tensor> {
        unsafe {
            let mut t = Tensor::uninitialized::<T>(shape)?;
            t.clear::<T>()?;
            Ok(t)
        }
    }
    pub fn fill_t<T: Datum + Clone>(&mut self, value: T) -> anyhow::Result<()> {
        self.as_slice_mut::<T>()?
            .iter_mut()
            .for_each(|item| *item = value.clone());
        Ok(())
    }
    pub fn zero_aligned<T: Datum>(shape: &[usize], alignment: usize) -> anyhow::Result<Tensor> {
        unsafe {
            let mut tensor = Self::uninitialized_aligned::<T>(shape, alignment)?;
            tensor.clear::<T>()?;
            Ok(tensor)
        }
    }
    #[inline]
    pub fn rank(&self) -> usize {
        self.shape.len()
    }
    #[inline]
    pub fn shape(&self) -> &[usize] {
        &self.shape
    }
    #[inline]
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.len
    }
    fn update_strides_and_len(&mut self) {
        self.strides.clear();
        compute_natural_stride_to(&mut self.strides, &self.shape);
        self.len = if self.rank() == 0 {
            1
        } else {
            unsafe { *self.strides.get_unchecked(0) as usize * self.shape.get_unchecked(0) }
        }
    }
    #[inline]
    pub fn datum_type(&self) -> DatumType {
        self.dt
    }
    #[inline]
    pub unsafe fn set_datum_type(&mut self, dt: DatumType) {
        self.dt = dt
    }
    fn check_for_access<D: Datum>(&self) -> anyhow::Result<()> {
        if self.datum_type().unquantized() != D::datum_type().unquantized() {
            unimplemented!()
        }
        Ok(())
    }
    pub fn as_ptr<D: Datum>(&self) -> anyhow::Result<*const D> {
        self.check_for_access::<D>()?;
        Ok(self.data as *const D)
    }
    pub fn as_ptr_mut<D: Datum>(&mut self) -> anyhow::Result<*mut D> {
        self.as_ptr::<D>().map(|p| p as *mut D)
    }
    pub fn as_slice_mut<D: Datum>(&mut self) -> anyhow::Result<&mut [D]> {
        let ptr: *mut D = self.as_ptr_mut()?;
        if ptr.is_null() {
            unimplemented!()
        } else {
            unsafe { Ok(std::slice::from_raw_parts_mut::<D>(ptr, self.len())) }
        }
    }
    pub unsafe fn as_slice_unchecked<D: Datum>(&self) -> &[D] {
        if self.data.is_null() {
            unimplemented!()
        } else {
            std::slice::from_raw_parts::<D>(self.data as *const D, self.len())
        }
    }
    unsafe fn is_uniform_t<T: Datum>(&self) -> bool {
        let slice = self.as_slice_unchecked::<T>();
        slice[1..].iter().all(|x| x == &slice[0])
    }
    pub fn is_uniform(&self) -> bool {
        if self.len() <= 1 {
            unimplemented!()
        }
        unsafe { Tensor::is_uniform_t::<f32>(self) }
    }
    unsafe fn as_uniform_t<T: Datum>(&self) -> Tensor {
        let v: T = self.as_slice_unchecked::<T>()[0].clone();
        tensor0(v)
    }
    pub fn as_uniform(&self) -> Option<Tensor> {
        if self.len() >= 1 && self.is_uniform() {
            unsafe {
                let mut t = Tensor::as_uniform_t::<f32>(self);
                t.set_datum_type(self.datum_type());
                Some(t)
            }
        } else {
            unimplemented!()
        }
    }
    fn from_datum<T: Datum>(it: ArrayD<T>) -> Tensor {
        if it.as_slice().is_some() {
            let shape = it.shape().into();
            let vec = it.into_raw_vec().into_boxed_slice();
            let data = Box::into_raw(vec) as *mut u8;
            let mut t = Tensor {
                dt: T::datum_type(),
                shape,
                data,
                strides: vec![],
                len: 0,
            };
            t.update_strides_and_len();
            return t;
        }
        unimplemented!()
    }
}
impl PartialEq for Tensor {
    fn eq(&self, _other: &Tensor) -> bool {
        unimplemented!()
    }
}
fn compute_natural_stride_to(strides: &mut Vec<isize>, shape: &[usize]) {
    match shape.len() {
        0 => (),
        1 => strides.push(1),
        2 => strides.extend_from_slice(&[shape[1] as isize, 1]),
        3 => strides.extend_from_slice(&[(shape[1] * shape[2]) as isize, shape[2] as _, 1]),
        4 => strides.extend_from_slice(&[
            (shape[1] * shape[2] * shape[3]) as isize,
            (shape[2] * shape[3]) as _,
            shape[3] as _,
            1,
        ]),
        _ => {
            unimplemented!()
        }
    }
}
impl<D: ::ndarray::Dimension, T: Datum> From<Array<T, D>> for Tensor {
    fn from(it: Array<T, D>) -> Tensor {
        Tensor::from_datum(it.into_dyn())
    }
}
pub trait IntoArcTensor: Sized {
    fn into_arc_tensor(self) -> Arc<Tensor>;
}
impl IntoArcTensor for Tensor {
    fn into_arc_tensor(self) -> Arc<Tensor> {
        Arc::new(self)
    }
}
impl IntoArcTensor for Arc<Tensor> {
    fn into_arc_tensor(self) -> Arc<Tensor> {
        self
    }
}
#[macro_use]
pub extern crate downcast_rs;
use std::sync::Arc;
    use downcast_rs::Downcast;
        macro_rules! as_op {
            () => {
                fn as_op(&self) -> &dyn Op {
                    self
                }
            };
        }
        #[derive(Clone, Default)]
        pub struct Dummy;
        impl Op for Dummy {}
        impl TypedOp for Dummy {
            as_op!();
            fn output_facts(&self, _inputs: &[&TypedFact]) -> TractResult<Vec<TypedFact>> {
                unimplemented!()
            }
        }
        #[derive(Clone)]
        pub struct Const(pub Arc<Tensor>);
        impl Op for Const {}
        impl TypedOp for Const {
            as_op!();
            fn output_facts(&self, _inputs: &[&TypedFact]) -> TractResult<Vec<TypedFact>> {
                unimplemented!()
            }
        }
            #[derive(Copy, Clone, PartialEq, Eq)]
            pub enum BinOp {
                Min,
                Max,
            }
            #[derive(Clone, Copy, Eq, PartialEq)]
            pub enum OutputStoreSpec {
                View {
                    m_axis: usize,
                },
                Strides {
                    col_byte_stride: isize,
                    mr: usize,
                    nr: usize,
                    m: usize,
                    n: usize,
                },
            }
            #[derive(PartialEq, Eq, Clone)]
            pub enum ProtoFusedSpec {
                BinScalar(AttrOrInput, BinOp),
                BinPerRow(AttrOrInput, BinOp),
                BinPerCol(AttrOrInput, BinOp),
                AddRowColProducts(AttrOrInput, AttrOrInput),
                AddUnicast(OutputStoreSpec, AttrOrInput),
                Store,
            }
            #[derive(Clone)]
            pub struct LirMatMulUnary {
                pub micro_ops: ArrayD<(Arc<Tensor>, Vec<ProtoFusedSpec>)>,
            }
            impl Op for LirMatMulUnary {}
            impl TypedOp for LirMatMulUnary {
                fn output_facts(&self, _inputs: &[&TypedFact]) -> TractResult<Vec<TypedFact>> {
                    Ok(vec!(f32::fact([1, 2])))
                }
                as_op!();
            }
            #[derive(Clone)]
            pub struct MatMul {}
            impl Op for MatMul {}
            impl TypedOp for MatMul {
                fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<Vec<TypedFact>> {
                    let (_m, _k, _n, c_shape) = compute_shape(&inputs[0].shape, &inputs[1].shape)?;
                    Ok(vec!(f32::fact(c_shape)))
                }
                fn declutter(
                    &self,
                    model: &TypedModel,
                    node: &TypedNode,
                ) -> TractResult<Option<TypedModelPatch>> {
                    let konst = model.outlet_fact(node.inputs[0])?.konst.clone().unwrap();
                    TypedModelPatch::replace_single_op(
                        model,
                        node,
                        &node.inputs[1..2],
                        MatMulUnary { a: konst },
                    )
                    .map(Some)
                }
                as_op!();
            }
            #[derive(Clone)]
            pub struct MatMulUnary {
                pub a: Arc<Tensor>,
            }
            impl Op for MatMulUnary {}
            impl TypedOp for MatMulUnary {
                fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<Vec<TypedFact>> {
                    let (_m, _k, _n, c_shape) = compute_shape(
                        &self
                            .a
                            .shape(),
                        &inputs[0].shape,
                    )?;
                    let c_dt = output_type(inputs[0].datum_type);
                    Ok(vec!(c_dt.fact(c_shape)))
                }
                fn codegen(
                    &self,
                    model: &TypedModel,
                    node: &TypedNode,
                ) -> TractResult<Option<TypedModelPatch>> {
                    let _patch = self.new_mat_mul_unary_finite(model, node)?;
                    Ok(None)
                }
                as_op!();
            }
            impl MatMulUnary {
                fn new_mat_mul_unary_finite(
                    &self,
                    model: &TypedModel,
                    node: &TypedNode,
                ) -> TractResult<TypedModelPatch> {
                    let mut patch = TypedModelPatch::default();
                    let mut wire = patch.tap_model(model, node.inputs[0])?;
                    let packed_as = Array::from_shape_fn(vec![1, 1], |_| {
                        let pa = Tensor::zero_aligned::<f32>(&[64], 32).unwrap();
                        (pa.into_arc_tensor(), vec![ProtoFusedSpec::Store])
                    });
                    wire = patch.wire_node(
                        format!("{}.pack", &*node.name),
                        MatMatMulPack {},
                        &[wire],
                    )?[0];
                    let op = LirMatMulUnary {
                        micro_ops: packed_as,
                    };
                    patch.wire_node(format!("{}.matmatmul", &*node.name), op, &[wire])?[0];
                    Ok(patch)
                }
            }
            #[derive(Clone, PartialEq, Eq)]
            pub struct MatMatMulPack {}
            impl Op for MatMatMulPack {}
            impl TypedOp for MatMatMulPack {
                fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<Vec<TypedFact>> {
                    Ok(vec!(inputs[0]
                        .datum_type
                        .fact(self.output_shape(&inputs[0].shape))))
                }
                as_op!();
            }
            impl MatMatMulPack {
                fn output_shape(&self, _input: &[usize]) -> Vec<usize> {
                    vec!(1)
                }
            }
        pub fn compute_shape(
            ashape: &[usize],
            bshape: &[usize],
        ) -> TractResult<(usize, usize, usize, Vec<usize>)> {
            let a_shape_bc: Vec<usize> = vec!();
            let b_shape_bc = vec!();
            let mut c_shape = multi_broadcast(&[a_shape_bc, b_shape_bc]).unwrap();
            let (m, ka) = (ashape[0].clone(), ashape[1].clone());
            let (_, n) = (bshape[0].clone(), bshape[1].clone());
            c_shape.insert(0, n.clone());
            c_shape.insert(1, m.clone());
            Ok((m, ka, n, c_shape))
        }
        pub fn output_type(input: DatumType) -> DatumType {
            input
        }
        #[derive(Clone)]
        pub struct TypedSource {}
        impl Op for TypedSource {}
        impl TypedOp for TypedSource {
            fn output_facts(&self, _inputs: &[&TypedFact]) -> TractResult<Vec<TypedFact>> {
                unimplemented!()
            }
            as_op!();
        }
    pub trait Op: dyn_clone::DynClone + Send + Sync + 'static + Downcast {}
    pub trait TypedOp: Op + dyn_clone::DynClone + Send + Sync + 'static + Downcast {
        fn as_op(&self) -> &dyn Op;
        fn output_facts(&self, inputs: &[&TypedFact]) -> TractResult<Vec<TypedFact>>;
        #[allow(unused_variables)]
        fn declutter_with_session(
            &self,
            session: &mut OptimizerSession,
            model: &TypedModel,
            node: &TypedNode,
        ) -> TractResult<Option<TypedModelPatch>> {
            self.declutter(model, node)
        }
        #[allow(unused_variables)]
        fn declutter(
            &self,
            model: &TypedModel,
            node: &TypedNode,
        ) -> TractResult<Option<TypedModelPatch>> {
            Ok(None)
        }
        #[allow(unused_variables)]
        fn codegen(
            &self,
            model: &TypedModel,
            node: &TypedNode,
        ) -> TractResult<Option<TypedModelPatch>> {
            Ok(None)
        }
        #[allow(unused_variables)]
        fn nested_model_multipliers(&self, inputs: &[&TypedFact]) -> Vec<(Cow<str>, f64)> {
            unimplemented!()
        }
    }
    impl_downcast!(Op);
    dyn_clone::clone_trait_object!(TypedOp);
    impl<O: TypedOp> From<O> for Box<dyn TypedOp> {
        fn from(it: O) -> Box<dyn TypedOp> {
            Box::new(it)
        }
    }
    impl<'a> From<&'a Box<dyn TypedOp>> for Box<dyn TypedOp> {
        fn from(it: &'a Box<dyn TypedOp>) -> Box<dyn TypedOp> {
            it.clone()
        }
    }
    impl AsRef<dyn Op> for Box<dyn TypedOp> {
        fn as_ref(&self) -> &dyn Op {
            unimplemented!()
        }
    }
    impl AsMut<dyn Op> for Box<dyn TypedOp> {
        fn as_mut(&mut self) -> &mut dyn Op {
            unimplemented!()
        }
    }
    #[derive(Clone, PartialEq, Eq)]
    pub enum AttrOrInput {
        Attr(Arc<Tensor>),
        Input(usize),
    }
    pub fn multi_broadcast(_shapes: &[impl AsRef<[usize]>]) -> Option<Vec<usize>>
    {
        Some(vec!())
    }
        #[derive(Clone, PartialEq, Eq)]
        pub struct ShapeFact {
            dims: Vec<usize>,
            concrete: Option<Vec<usize>>,
        }
        impl ShapeFact {
            fn compute_concrete(&mut self) {
                self.concrete = Some(self.dims.clone())
            }
            pub fn from_dims<T: IntoIterator<Item = usize>>(it: T) -> ShapeFact {
                let mut dims = ShapeFact {
                    dims: it.into_iter().collect(),
                    concrete: None,
                };
                dims.compute_concrete();
                dims
            }
        }
        impl std::ops::Deref for ShapeFact {
            type Target = [usize];
            fn deref(&self) -> &[usize] {
                &self.dims
            }
        }
        impl<T: IntoIterator<Item = usize>> From<T> for ShapeFact {
            fn from(it: T) -> ShapeFact {
                ShapeFact::from_dims(it)
            }
        }
        #[derive(Clone, PartialEq, Eq)]
        pub struct TypedFact {
            pub datum_type: DatumType,
            pub shape: ShapeFact,
            pub konst: Option<Arc<Tensor>>,
            pub uniform: Option<Arc<Tensor>>,
        }
        impl TypedFact {
            pub fn shape<T, S>(shape: S) -> TypedFact
            where
                T: Datum,
                S: Into<ShapeFact>,
            {
                Self::dt_shape(T::datum_type(), shape)
            }
            pub fn dt_shape<S>(datum_type: DatumType, shape: S) -> TypedFact
            where
                S: Into<ShapeFact>,
            {
                TypedFact {
                    datum_type,
                    shape: shape.into(),
                    konst: None,
                    uniform: None,
                }
            }
        }
        impl From<Arc<Tensor>> for TypedFact {
            fn from(t: Arc<Tensor>) -> TypedFact {
                TypedFact {
                    datum_type: t.datum_type(),
                    shape: ShapeFact::from_dims(t.shape().to_vec()),
                    uniform: t.as_uniform().map(Arc::new),
                    konst: Some(t),
                }
            }
        }
        impl<'a> From<&'a TypedFact> for TypedFact {
            fn from(fact: &TypedFact) -> TypedFact {
                fact.clone()
            }
        }
        pub trait DatumExt {
            fn scalar_fact() -> TypedFact;
            fn fact<S>(shape: S) -> TypedFact
            where
                S: Into<ShapeFact>;
        }
        impl<T: Datum> DatumExt for T {
            #[allow(clippy::needless_borrow)]
            fn scalar_fact() -> TypedFact {
                unimplemented!()
            }
            fn fact<S>(shape: S) -> TypedFact
            where
                S: Into<ShapeFact>,
            {
                TypedFact::shape::<Self, _>(shape)
            }
        }
        pub trait DatumTypeExt {
            fn scalar_fact(&self) -> TypedFact;
            fn fact<S>(&self, shape: S) -> TypedFact
            where
                S: Into<ShapeFact>;
        }
        impl DatumTypeExt for DatumType {
            #[allow(clippy::needless_borrow)]
            fn scalar_fact(&self) -> TypedFact {
                unimplemented!()
            }
            fn fact<S>(&self, shape: S) -> TypedFact
            where
                S: Into<ShapeFact>,
            {
                TypedFact::dt_shape(*self, shape)
            }
        }
        pub trait SpecialOps<F, O> {
            fn create_dummy(&self) -> O;
            fn create_source(&self, fact: F) -> O;
            fn is_source(op: &O) -> bool;
            fn wire_node(
                &mut self,
                name: impl Into<String>,
                op: impl Into<O>,
                inputs: &[OutletId],
            ) -> TractResult<Vec<OutletId>>;
        }
        #[derive(Clone)]
        pub struct Graph<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            pub nodes: Vec<Node<F, O>>,
            pub inputs: Vec<OutletId>,
            pub outputs: Vec<OutletId>,
            pub outlet_labels: HashMap<OutletId, String>,
        }
        impl<F, O> Default for Graph<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            fn default() -> Graph<F, O> {
                Graph {
                    nodes: vec![],
                    inputs: vec![],
                    outputs: vec![],
                    outlet_labels: HashMap::new(),
                }
            }
        }
        impl<F, O> Graph<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
            Graph<F, O>: SpecialOps<F, O>,
        {
            pub fn add_source(
                &mut self,
                name: impl Into<String>,
                fact: F,
            ) -> TractResult<OutletId> {
                let source = self.create_source(fact.clone());
                let id = self.add_node(name, source, vec!(fact))?;
                let id = OutletId::new(id, 0);
                self.inputs.push(id);
                Ok(id)
            }
        }
        impl<F, O> Graph<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            pub fn add_node(
                &mut self,
                name: impl Into<String>,
                op: impl Into<O>,
                output_facts: Vec<F>,
            ) -> TractResult<usize> {
                let op = op.into();
                let name = name.into();
                let id = self.nodes.len();
                let outputs = output_facts
                    .into_iter()
                    .map(|fact| Outlet {
                        fact,
                        successors: vec!(),
                    })
                    .collect();
                let node = Node {
                    id,
                    name,
                    op,
                    inputs: vec![],
                    outputs,
                };
                self.nodes.push(node);
                Ok(id)
            }
            pub fn add_edge(&mut self, outlet: OutletId, inlet: InletId) -> TractResult<()> {
                {
                    let prec = &mut self.nodes[outlet.node];
                    prec.outputs[outlet.slot].successors.push(inlet);
                }
                let succ = &mut self.nodes[inlet.node];
                #[allow(clippy::comparison_chain)]
                if inlet.slot == succ.inputs.len() {
                    succ.inputs.push(outlet);
                }
                Ok(())
            }
            pub fn input_outlets(&self) -> TractResult<&[OutletId]> {
                Ok(&self.inputs)
            }
            pub fn set_input_outlets(&mut self, inputs: &[OutletId]) -> TractResult<()> {
                self.inputs = inputs.to_vec();
                Ok(())
            }
            pub fn output_outlets(&self) -> TractResult<&[OutletId]> {
                Ok(&self.outputs)
            }
            pub fn set_output_outlets(&mut self, outputs: &[OutletId]) -> TractResult<()> {
                self.outputs = outputs.to_vec();
                Ok(())
            }
            pub fn node(&self, id: usize) -> &Node<F, O> {
                &self.nodes[id]
            }
            pub fn node_mut(&mut self, id: usize) -> &mut Node<F, O> {
                &mut self.nodes[id]
            }
            pub fn nodes(&self) -> &[Node<F, O>] {
                &self.nodes
            }
            pub fn outlet_fact(&self, outlet: OutletId) -> TractResult<&F> {
                let outlets = &self.nodes[outlet.node].outputs;
                Ok(outlets.get(outlet.slot).map(|o| &o.fact).unwrap())
            }
            pub fn eval_order(&self) -> TractResult<Vec<usize>> {
                eval_order(self)
            }
        }
        impl<F: Clone + 'static, O> Graph<F, O>
        where
            F: Clone + 'static + From<std::sync::Arc<Tensor>>,
            O: From<Const> + AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            pub fn add_const(
                &mut self,
                name: impl Into<String>,
                v: impl IntoArcTensor,
            ) -> TractResult<OutletId> {
                let v = v.into_arc_tensor();
                let fact = F::from(v.clone());
                let name = name.into();
                self.add_node(name, Const(v), vec!(fact))
                    .map(|id| id.into())
            }
        }
        impl<F, O> Graph<F, O>
        where
            F: Clone + 'static + for<'a> std::convert::From<&'a F>,
            O: Clone
                + AsRef<dyn Op>
                + AsMut<dyn Op>
                + Clone
                + 'static
                + for<'a> std::convert::From<&'a O>,
            Graph<F, O>: SpecialOps<F, O>,
        {
            pub fn compact(&mut self) -> TractResult<()> {
                let mut result = IntoTranslator.translate_model(self)?;
                std::mem::swap(self, &mut result);
                Ok(())
            }
        }
        #[derive(Clone)]
        pub struct Node<F, O> {
            pub id: usize,
            pub name: String,
            pub inputs: Vec<OutletId>,
            #[cfg_attr(feature = "serialize", serde(skip))]
            pub op: O,
            pub outputs: Vec<Outlet<F>>,
        }
        #[derive(Clone, Default)]
        pub struct Outlet<F> {
            pub fact: F,
            pub successors: Vec<InletId>,
        }
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct OutletId {
            pub node: usize,
            pub slot: usize,
        }
        impl OutletId {
            pub fn new(node: usize, slot: usize) -> OutletId {
                OutletId { node, slot }
            }
        }
        impl From<usize> for OutletId {
            fn from(node: usize) -> OutletId {
                OutletId::new(node, 0)
            }
        }
        impl From<(usize, usize)> for OutletId {
            fn from(pair: (usize, usize)) -> OutletId {
                OutletId::new(pair.0, pair.1)
            }
        }
        #[derive(Clone, Copy, PartialEq, Eq, Ord, PartialOrd)]
        pub struct InletId {
            pub node: usize,
            pub slot: usize,
        }
        pub fn eval_order<F, O>(model: &Graph<F, O>) -> TractResult<Vec<usize>>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            let inputs = model
                .input_outlets()?
                .iter()
                .map(|n| n.node)
                .collect::<Vec<usize>>();
            let targets = model
                .output_outlets()?
                .iter()
                .map(|n| n.node)
                .collect::<Vec<usize>>();
            eval_order_for_nodes(model.nodes(), &inputs, &targets, &[])
        }
        pub fn eval_order_for_nodes<F, O>(
            nodes: &[Node<F, O>],
            model_inputs: &[usize],
            model_outputs: &[usize],
            more_dependencies: &[(usize, usize)],
        ) -> TractResult<Vec<usize>>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            let mut done = std::collections::HashSet::new();
            let mut order: Vec<usize> = vec![];
            for &model_target in model_outputs {
                if done.contains(&model_target) {
                    unimplemented!()
                }
                let mut current_stack: Vec<(usize, usize)> = vec![(model_target, 0)];
                let mut pending = std::collections::HashSet::new();
                while let Some((current_node, current_input)) = current_stack.pop() {
                    let deps_from_inputs = nodes[current_node].inputs.len();
                    let all_deps_count = deps_from_inputs
                        + more_dependencies
                            .iter()
                            .filter(|a| a.0 == current_node)
                            .count();
                    if model_inputs.contains(&current_node) || current_input == all_deps_count {
                        order.push(current_node);
                        done.insert(current_node);
                        pending.remove(&current_node);
                    } else {
                        let precursor: usize = nodes[current_node]
                            .inputs
                            .iter()
                            .filter(|n| nodes[n.node].inputs.len() > 0)
                            .map(|n| n.node)
                            .chain(
                                more_dependencies
                                    .iter()
                                    .filter(|a| a.0 == current_node)
                                    .map(|n| n.1),
                            )
                            .chain(
                                nodes[current_node]
                                    .inputs
                                    .iter()
                                    .filter(|n| nodes[n.node].inputs.len() == 0)
                                    .map(|n| n.node),
                            )
                            .nth(current_input)
                            .unwrap();
                        if done.contains(&precursor) {
                            current_stack.push((current_node, current_input + 1));
                        } else if pending.contains(&precursor) {
                            unimplemented!()
                        } else {
                            pending.insert(precursor);
                            current_stack.push((current_node, current_input));
                            current_stack.push((precursor, 0));
                        }
                    }
                }
            }
            Ok(order)
        }
        use std::ops::{Deref, DerefMut};
        #[derive(Clone)]
        pub struct ModelPatch<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            pub dont_apply_twice: Option<String>,
            pub model: Graph<F, O>,
            pub inputs: HashMap<usize, usize>,
            pub incoming: HashMap<OutletId, OutletId>,
            pub shunt_outlet_by: HashMap<OutletId, OutletId>,
            pub obliterate: Vec<usize>,
        }
        impl<F, O> Default for ModelPatch<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            fn default() -> ModelPatch<F, O> {
                ModelPatch {
                    dont_apply_twice: None,
                    model: Graph::default(),
                    inputs: HashMap::default(),
                    incoming: HashMap::new(),
                    shunt_outlet_by: HashMap::new(),
                    obliterate: vec![],
                }
            }
        }
        impl<F, O> Deref for ModelPatch<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            type Target = Graph<F, O>;
            fn deref(&self) -> &Graph<F, O> {
                unimplemented!()
            }
        }
        impl<F, O> DerefMut for ModelPatch<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            fn deref_mut(&mut self) -> &mut Graph<F, O> {
                &mut self.model
            }
        }
        impl<F, O> ModelPatch<F, O>
        where
            F: Clone + 'static,
            O: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
            Graph<F, O>: SpecialOps<F, O>,
        {
            pub fn tap_model(
                &mut self,
                model: &Graph<F, O>,
                outlet: OutletId,
            ) -> TractResult<OutletId> {
                let fact = model.outlet_fact(outlet)?;
                let id = self.add_source(
                    format!("incoming-{}/{}", outlet.node, outlet.slot),
                    dyn_clone::clone(fact),
                )?;
                self.incoming.insert(id, outlet);
                Ok(id)
            }
            pub fn shunt_outside(&mut self, outlet: OutletId, by: OutletId) -> TractResult<()> {
                self.shunt_outlet_by.insert(outlet, by);
                Ok(())
            }
            pub fn obliterate(&mut self, node: usize) -> TractResult<()> {
                self.obliterate.push(node);
                Ok(())
            }
            pub fn replace_single_op<IO: Into<O>>(
                patched_model: &Graph<F, O>,
                node: &Node<F, O>,
                inputs: &[OutletId],
                new_op: IO,
            ) -> TractResult<ModelPatch<F, O>> {
                let mut patch = ModelPatch::default();
                let new_op = new_op.into();
                let inputs = inputs
                    .iter()
                    .map(|i| patch.tap_model(patched_model, *i))
                    .collect::<TractResult<Vec<_>>>()?;
                let wires = patch.wire_node(&node.name, new_op, &inputs)?;
                for (ix, o) in wires.iter().enumerate() {
                    patch.shunt_outside(OutletId::new(node.id, ix), *o)?;
                }
                patch.obliterate(node.id)?;
                Ok(patch)
            }
            pub fn apply(self, target: &mut Graph<F, O>) -> TractResult<()> {
                let prior_target_inputs = target.input_outlets()?.len();
                let prior_target_outputs = target.output_outlets()?.len();
                let ModelPatch {
                    model: patch,
                    incoming: mut mapping,
                    shunt_outlet_by,
                    obliterate,
                    ..
                } = self;
                let mut all_inputs = HashMap::new();
                let model_input_outlets = target.input_outlets()?.to_vec();
                for node in patch.nodes {
                    if <Graph<F, O>>::is_source(&node.op)
                        && mapping.contains_key(&OutletId::new(node.id, 0))
                    {
                        continue;
                    }
                    let Node {
                        id: patch_node_id,
                        name,
                        inputs,
                        op,
                        outputs,
                    } = node;
                    let n_outputs = outputs.len();
                    let facts = outputs.into_iter().map(|of| of.fact).collect();
                    let added_node_id = target.add_node(name, op, facts)?;
                    for ix in 0..n_outputs {
                        mapping.insert(
                            OutletId::new(patch_node_id, ix),
                            OutletId::new(added_node_id, ix),
                        );
                    }
                    all_inputs.insert(added_node_id, inputs);
                }
                debug_assert_eq!(target.input_outlets()?.len(), prior_target_inputs);
                debug_assert_eq!(target.output_outlets()?.len(), prior_target_outputs);
                for (outlet, by) in shunt_outlet_by {
                    let replace_by = mapping[&by];
                    for o in target.outputs.iter_mut() {
                        if *o == outlet {
                            *o = replace_by;
                        }
                    }
                }
                debug_assert_eq!(target.input_outlets()?.len(), prior_target_inputs);
                debug_assert_eq!(target.output_outlets()?.len(), prior_target_outputs);
                for (node, inputs) in all_inputs {
                    for (ix, input) in inputs.into_iter().enumerate() {
                        target.add_edge(mapping[&input], InletId { node, slot: ix })?;
                    }
                }
                debug_assert_eq!(target.input_outlets()?.len(), prior_target_inputs);
                debug_assert_eq!(target.output_outlets()?.len(), prior_target_outputs);
                for node in obliterate {
                    target.node_mut(node).op = target.create_dummy();
                }
                debug_assert_eq!(target.input_outlets()?.len(), prior_target_inputs);
                debug_assert_eq!(target.output_outlets()?.len(), prior_target_outputs);
                target.set_input_outlets(&model_input_outlets)?;
                Ok(())
            }
        }
        pub trait Translate<TI1, O1, TI2, O2>
        where
            TI1: Clone + 'static,
            TI2: Clone + 'static,
            O1: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
            O2: AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
        {
            fn translate_node(
                &self,
                source: &Graph<TI1, O1>,
                node: &Node<TI1, O1>,
                target: &mut Graph<TI2, O2>,
                mapping: &HashMap<OutletId, OutletId>,
            ) -> TractResult<Vec<OutletId>>;
            fn translate_model(&self, source: &Graph<TI1, O1>) -> TractResult<Graph<TI2, O2>> {
                Ok(self.translate_model_with_mappings(source)?.0)
            }
            fn translate_model_with_mappings(
                &self,
                source: &Graph<TI1, O1>,
            ) -> TractResult<(Graph<TI2, O2>, HashMap<OutletId, OutletId>)> {
                let mut target = Graph::default();
                let mut mapping = HashMap::new();
                for old_id in source.eval_order()? {
                    let node = source.node(old_id);
                    let outlets = self.translate_node(source, node, &mut target, &mapping)?;
                    for (ix, outlet) in outlets.into_iter().enumerate() {
                        mapping.insert(OutletId::new(node.id, ix), outlet);
                    }
                }
                target.inputs = source.input_outlets()?.iter().map(|i| mapping[i]).collect();
                target.outputs = source
                    .output_outlets()?
                    .iter()
                    .map(|o| mapping[o])
                    .collect();
                Ok((target, mapping))
            }
        }
        pub struct IntoTranslator;
        impl<TI1, O1, TI2, O2, EO, ETI> Translate<TI1, O1, TI2, O2> for IntoTranslator
        where
            TractError: From<EO> + From<ETI>,
            TI1: Clone + 'static,
            TI2: for<'a> TryFrom<&'a TI1, Error = EO> + Clone + 'static,
            O1: Clone + AsRef<dyn Op> + AsMut<dyn Op> + Clone + 'static,
            O2: for<'a> TryFrom<&'a O1, Error = ETI>
                + AsRef<dyn Op>
                + AsMut<dyn Op>
                + Clone
                + 'static,
            Graph<TI2, O2>: SpecialOps<TI2, O2>,
        {
            fn translate_node(
                &self,
                source: &Graph<TI1, O1>,
                node: &Node<TI1, O1>,
                target: &mut Graph<TI2, O2>,
                mapping: &HashMap<OutletId, OutletId>,
            ) -> TractResult<Vec<OutletId>> {
                let node_is_input =
                    (0..node.outputs.len()).all(|o| source.inputs.contains(&(node.id, o).into()));
                if node_is_input {
                    (0..node.outputs.len())
                        .map(|i| {
                            target.add_source(
                                if node.outputs.len() > 1 {
                                    unimplemented!()
                                } else {
                                    node.name.to_string()
                                },
                                TI2::try_from(&node.outputs[i].fact)?,
                            )
                        })
                        .collect()
                } else {
                    let new_op = O2::try_from(&node.op)?;
                    let facts = node
                        .outputs
                        .iter()
                        .map(|of| Ok(TI2::try_from(&of.fact)?))
                        .collect::<TractResult<Vec<_>>>()?;
                    let new_id = target.add_node(node.name.clone(), new_op, facts)?;
                    for (ix, o) in node.inputs.iter().enumerate() {
                        target.add_edge(
                            mapping[o],
                            InletId {
                                node: new_id,
                                slot: ix,
                            },
                        )?
                    }
                    Ok(node
                        .outputs
                        .iter()
                        .enumerate()
                        .map(|(ix, _)| OutletId::new(new_id, ix))
                        .collect())
                }
            }
        }
        pub type TypedModel = Graph<TypedFact, Box<dyn TypedOp>>;
        pub type TypedNode = Node<TypedFact, Box<dyn TypedOp>>;
        pub type TypedModelPatch = ModelPatch<TypedFact, Box<dyn TypedOp>>;
        impl SpecialOps<TypedFact, Box<dyn TypedOp>> for TypedModel {
            fn is_source(op: &Box<dyn TypedOp>) -> bool {
                op.as_op()
                    .downcast_ref::<TypedSource>()
                    .is_some()
            }
            fn create_dummy(&self) -> Box<dyn TypedOp> {
                Box::new(Dummy::default())
            }
            fn create_source(&self, _fact: TypedFact) -> Box<dyn TypedOp> {
                Box::new(TypedSource {})
            }
            fn wire_node(
                &mut self,
                name: impl Into<String>,
                op: impl Into<Box<dyn TypedOp>>,
                inputs: &[OutletId],
            ) -> TractResult<Vec<OutletId>> {
                let op = op.into();
                let name = name.into();
                {
                    let output_facts = || -> TractResult<Vec<TypedFact>> {
                        let input_facts = inputs
                            .iter()
                            .map(|o| self.outlet_fact(*o))
                            .collect::<TractResult<Vec<_>>>()?;
                        let facts = op.output_facts(&input_facts)?;
                        Ok(facts)
                    };
                    let output_facts = output_facts()?;
                    let id = self.add_node(&name, &op, output_facts)?;
                    inputs.iter().enumerate().try_for_each(|(ix, i)| {
                        self.add_edge(*i, InletId { node: id, slot: ix })
                    })?;
                    TractResult::Ok(
                        self.node(id)
                            .outputs
                            .iter()
                            .enumerate()
                            .map(|(ix, _)| OutletId::new(id, ix))
                            .collect(),
                    )
                }
            }
        }
        impl TypedModel {
            pub fn into_optimized(mut self) -> TractResult<TypedModel> {
                self.declutter()?;
                self.optimize()?;
                Ok(self)
            }
            pub fn into_decluttered(mut self) -> TractResult<TypedModel> {
                self.declutter()?;
                Ok(self)
            }
            pub fn declutter(&mut self) -> TractResult<()> {
                Optimizer::declutter()
                    .session()
                    .optimize(self)
            }
            pub fn optimize(&mut self) -> TractResult<()> {
                Optimizer::codegen().optimize(self)
            }
        }
        #[derive(Clone)]
        pub struct OpOptim(
            pub &'static str,
            pub  fn(
                op: &dyn TypedOp,
                session: &mut OptimizerSession,
                model: &TypedModel,
                node: &TypedNode,
            ) -> TractResult<Option<TypedModelPatch>>,
            pub usize,
        );
        impl OpOptim {
            fn full_pass(
                &mut self,
                session: &mut OptimizerSession,
                new: &TypedModel,
            ) -> TractResult<Option<TypedModelPatch>> {
                for (ix, &id) in new.eval_order()?.iter().enumerate().skip(self.2) {
                    let node = &new.nodes()[id];
                    let patch = (self.1)(node.op.as_ref(), session, new, node)?;
                    if let Some(p) = patch {
                        self.2 = ix + p.dont_apply_twice.is_some() as usize;
                        return Ok(Some(p));
                    }
                }
                Ok(None)
            }
        }
        impl TypedPass for OpOptim {
            fn reset(&mut self) -> TractResult<()> {
                self.2 = 0;
                Ok(())
            }
            fn next(
                &mut self,
                session: &mut OptimizerSession,
                model: &TypedModel,
            ) -> TractResult<Option<TypedModelPatch>> {
                self.full_pass(session, model)
            }
        }
    pub trait TypedPass: Send + Sync + dyn_clone::DynClone {
        fn reset(&mut self) -> TractResult<()>;
        fn next(
            &mut self,
            session: &mut OptimizerSession,
            model: &TypedModel,
        ) -> TractResult<Option<TypedModelPatch>>;
    }
    dyn_clone::clone_trait_object!(TypedPass);
    pub struct Optimizer {
        passes: Vec<Box<dyn TypedPass>>,
    }
    impl Optimizer {
        fn passes(passes: Vec<Box<dyn TypedPass>>) -> Optimizer {
            Optimizer { passes }
        }
        pub fn declutter() -> Optimizer {
            Optimizer::passes(vec![Box::new(OpOptim(
                "declutter",
                TypedOp::declutter_with_session,
                0,
            ))])
        }
        pub fn codegen() -> Optimizer {
            Optimizer::passes(vec![
                Box::new(OpOptim(
                    "codegen",
                    |op, _session, model, node| TypedOp::codegen(op, model, node),
                    0,
                )),
                Box::new(OpOptim("declutter", TypedOp::declutter_with_session, 0)),
            ])
        }
        pub fn optimize(&self, model: &mut TypedModel) -> TractResult<()> {
            self.session().optimize(model)
        }
        pub fn session(&self) -> OptimizerSession {
            OptimizerSession {
                optimizer: self,
                counter: 0,
            }
        }
    }
    pub struct OptimizerSession<'o> {
        optimizer: &'o Optimizer,
        counter: usize,
    }
    impl<'o> OptimizerSession<'o> {
        pub fn optimize(&mut self, model: &mut TypedModel) -> TractResult<()> {
            model.compact()?;
            for _i in 0.. {
                let old = self.counter;
                self.run_all_passes(model)?;
                if old == self.counter {
                    return Ok(());
                }
                model.compact()?;
            }
            unreachable!()
        }
        pub fn run_all_passes(&mut self, model: &mut TypedModel) -> TractResult<()> {
            let mut passes = self.optimizer.passes.clone();
            for p in passes.iter_mut() {
                self.run_one_pass_outer(p.as_mut(), model)?;
                model.compact()?;
            }
            Ok(())
        }
        pub fn run_one_pass_outer(
            &mut self,
            p: &mut dyn TypedPass,
            model: &mut TypedModel,
        ) -> TractResult<()> {
            loop {
                let old_counter = self.counter;
                self.run_one_pass_inner(p, model)?;
                if self.counter == old_counter {
                    return Ok(());
                }
                model.compact()?;
            }
        }
        pub fn run_one_pass_inner(
            &mut self,
            p: &mut dyn TypedPass,
            model: &mut TypedModel,
        ) -> TractResult<()> {
            p.reset()?;
            while let Some(patch) = p.next(self, model)? {
                patch.apply(model)?;
                self.counter += 1;
            }
            Ok(())
        }
    }
    pub use std::borrow::Cow;
    pub use std::collections::HashMap;

#[test]
fn crasher_monterey_matmul() {
    let mut model = TypedModel::default();
    let wire = model.add_source("input", f32::fact([1usize, 1])).unwrap();
    let a = model
        .add_const("a", Tensor::zero::<f32>(&[2, 1]).unwrap().into_arc_tensor())
        .unwrap();
    let op = MatMul {};
    let wire = model.wire_node("conv", op, &[a, wire]).unwrap()[0];
    model.set_output_outlets(&[wire]).unwrap();
    let decluttered = model.into_decluttered().unwrap();
    let _optimized = decluttered.into_optimized().unwrap();
}
