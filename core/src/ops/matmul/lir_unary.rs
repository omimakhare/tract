use crate::internal::*;
use ndarray::*;
use tract_itertools::Itertools;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum BinOp {
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
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

#[derive(PartialEq, Eq, Clone, Hash, Debug)]
pub enum ProtoFusedSpec {
    BinScalar(AttrOrInput, BinOp),
    BinPerRow(AttrOrInput, BinOp),
    BinPerCol(AttrOrInput, BinOp),
    AddRowColProducts(AttrOrInput, AttrOrInput),
    AddUnicast(OutputStoreSpec, AttrOrInput),
    Store,
}

#[derive(Clone, Debug, Hash)]
pub struct LirMatMulUnary {
    pub micro_ops: ArrayD<(Arc<Tensor>, Vec<ProtoFusedSpec>)>,
}

impl DynHash for LirMatMulUnary {
    fn dyn_hash(&self, hasher: &mut dyn std::hash::Hasher) {
        dyn_hash(self, hasher)
    }
}

impl Op for LirMatMulUnary {
    fn name(&self) -> Cow<str> {
        "LirMatMulUnary".into()
    }

    op_as_typed_op!();
}

impl EvalOp for LirMatMulUnary {
    fn is_stateless(&self) -> bool {
true
    }

    fn eval(&self, inputs: TVec<TValue>) -> TractResult<TVec<TValue>> {
panic!()
}
}

impl TypedOp for LirMatMulUnary {
    fn output_facts(&self, _inputs: &[&TypedFact]) -> TractResult<TVec<TypedFact>> {
        Ok(tvec!(f32::fact([1,2])))
    }

    as_op!();
}

#[test]
fn kali() {
	let mut patch = TypedModel::default();
	let mut wire = patch.add_source("x", f32::fact([1,1])).unwrap();

	let packed_as = Array::from_shape_fn(vec![1, 1], |_| {
	    let pa = Tensor::zero_aligned::<f32>(&[64], 32).unwrap();
	    (pa.into_arc_tensor(), vec![ ProtoFusedSpec::Store, ])
	});

	wire = patch.wire_node("pack", super::MatMatMulPack { }, &[wire],).unwrap()[0];

	let op = LirMatMulUnary { micro_ops: packed_as, };
	wire = patch.wire_node("matmatmul", op, &[wire]).unwrap()[0];
std::mem::drop(patch);
}
