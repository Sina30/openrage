use serde::{Deserialize, Serialize};

pub type TypeName = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BitsetStorage {
    Fixed8,
    Fixed16,
    Fixed32,
    Fixed64,
    Variable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum StringKind {
    #[default]
    Plain,
    AtHashString,
    AtHashValue,
    AtString,
    AtFinalHashString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PointerPolicy {
    #[default]
    Owner,
    Reference,
    ExternalNamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ArrayKind {
    #[default]
    AtArray,
    AtFixedArray(u32),
    AtPtrArray,
    /// A raw `T[N]` the enclosing struct holds inline.
    ///
    /// ⚠ **Zero means the PSC stated a size this parser could not evaluate**,
    /// not that the array is empty. 164 of the 486 `size=` attributes in the
    /// 1,134-file corpus are C++ constant expressions the PSC never defines —
    /// `CWeaponInfo::MAX_ATTACH_POINTS`, and one that multiplies two of them —
    /// so the header they come from would have to be parsed to resolve them.
    /// `AtFixedArray(0)` means the same thing.
    Member(u32),
    /// A `T*` the struct holds, pointing at a run another member counts.
    ///
    /// ⚠ **Not an `atArray`**: there is no header, so the count lives in
    /// `size_var` and the pointer is the whole member. `align` is the PSC's
    /// `align=`, in bytes, and zero where it states none.
    Pointer {
        size_var: Option<String>,
        align: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FieldType {
    Bool,
    U8,
    U16,
    U32,
    U64,
    S8,
    S16,
    S32,
    S64,
    Float,
    Float16,
    Double,
    String(StringKind),
    Vector2,
    Vector3,
    Vector4,
    Vec2V,
    Vec3V,
    Vec4V,
    Mat33V,
    Mat34V,
    Mat44V,
    Matrix33,
    Matrix34,
    Matrix44,
    Color32,
    Enum(TypeName),
    Bitset {
        storage: BitsetStorage,
        values: TypeName,
    },
    Struct(TypeName),
    Pointer {
        base: TypeName,
        policy: PointerPolicy,
    },
    Array {
        elem: Box<FieldType>,
        kind: ArrayKind,
    },
    Map {
        key: Box<FieldType>,
        value: Box<FieldType>,
    },
    Pad(u32),
    Unknown(String),
}

impl FieldType {
    pub fn is_numeric(&self) -> bool {
        matches!(
            self,
            FieldType::U8
                | FieldType::U16
                | FieldType::U32
                | FieldType::U64
                | FieldType::S8
                | FieldType::S16
                | FieldType::S32
                | FieldType::S64
                | FieldType::Float
                | FieldType::Float16
                | FieldType::Double
        )
    }

    pub fn is_integer(&self) -> bool {
        matches!(
            self,
            FieldType::U8
                | FieldType::U16
                | FieldType::U32
                | FieldType::U64
                | FieldType::S8
                | FieldType::S16
                | FieldType::S32
                | FieldType::S64
        )
    }

    pub fn is_float(&self) -> bool {
        matches!(
            self,
            FieldType::Float | FieldType::Float16 | FieldType::Double
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum FieldHint {
    #[default]
    None,
    ModelName,
    TextureName,
    ClipSetName,
    EffectRuleName,
    SoundName,
    ScenarioName,
    BoneTag,
    VehicleMod,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Literal {
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Enum(String),
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub ty: FieldType,
    pub init: Option<Literal>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub ui_key: bool,
    pub hide_widgets: bool,
    pub description: Option<String>,
    pub hint: FieldHint,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructDef {
    pub name: TypeName,
    pub base: Option<TypeName>,
    pub abstract_: bool,
    /// The PSC's `simple="true"`: no vtable, so members start at offset 0 and not 8.
    pub simple: bool,
    pub fields: Vec<Field>,
    pub source_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumValue {
    pub name: String,
    pub description: Option<String>,
    pub value: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnumDef {
    pub name: TypeName,
    pub values: Vec<EnumValue>,
    pub generate_bitset: bool,
    pub source_path: String,
}

impl EnumDef {
    pub fn variant_value(&self, idx: usize) -> i64 {
        if let Some(v) = self.values.get(idx).and_then(|v| v.value) {
            return v;
        }
        let mut next = 0i64;
        for (i, ev) in self.values.iter().enumerate() {
            if let Some(v) = ev.value {
                next = v;
            }
            if i == idx {
                return next;
            }
            next += 1;
        }
        idx as i64
    }
}
