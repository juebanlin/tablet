#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BaseType {
    Int,
    Long,
    Float,
    Double,
    Str,
    Bool,
}

impl BaseType {
    pub fn all() -> &'static [BaseType] {
        &[BaseType::Int, BaseType::Long, BaseType::Float, BaseType::Double, BaseType::Str, BaseType::Bool]
    }

    pub fn map_key_types() -> &'static [BaseType] {
        &[BaseType::Int, BaseType::Long, BaseType::Float, BaseType::Double, BaseType::Str]
    }

    pub fn name(&self) -> &'static str {
        match self {
            BaseType::Int => "int",
            BaseType::Long => "long",
            BaseType::Float => "float",
            BaseType::Double => "double",
            BaseType::Str => "str",
            BaseType::Bool => "bool",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "int" => Some(BaseType::Int),
            "long" => Some(BaseType::Long),
            "float" => Some(BaseType::Float),
            "double" => Some(BaseType::Double),
            "str" => Some(BaseType::Str),
            "bool" => Some(BaseType::Bool),
            _ => None,
        }
    }

    pub fn java_type(&self) -> &'static str {
        match self {
            BaseType::Int => "int",
            BaseType::Long => "long",
            BaseType::Float => "float",
            BaseType::Double => "double",
            BaseType::Str => "String",
            BaseType::Bool => "boolean",
        }
    }

    pub fn java_boxed(&self) -> &'static str {
        match self {
            BaseType::Int => "Integer",
            BaseType::Long => "Long",
            BaseType::Float => "Float",
            BaseType::Double => "Double",
            BaseType::Str => "String",
            BaseType::Bool => "Boolean",
        }
    }

    pub fn go_type(&self) -> &'static str {
        match self {
            BaseType::Int => "int32",
            BaseType::Long => "int64",
            BaseType::Float => "float32",
            BaseType::Double => "float64",
            BaseType::Str => "string",
            BaseType::Bool => "bool",
        }
    }

    pub fn lua_type(&self) -> &'static str {
        match self {
            BaseType::Int | BaseType::Long | BaseType::Float | BaseType::Double => "number",
            BaseType::Str => "string",
            BaseType::Bool => "boolean",
        }
    }

    pub fn validate_regex(&self) -> &'static str {
        match self {
            BaseType::Int => r"^-?\d+$",
            BaseType::Long => r"^-?\d+$",
            BaseType::Float | BaseType::Double => r"^-?\d+(\.\d+)?$",
            BaseType::Str => r"^.*$",
            BaseType::Bool => r"^(true|false)$",
        }
    }

    pub fn example_n(&self, idx: usize) -> &'static str {
        match (self, idx % 3) {
            (BaseType::Int, 0) => "1", (BaseType::Int, 1) => "2", (BaseType::Int, _) => "3",
            (BaseType::Long, 0) => "1000", (BaseType::Long, 1) => "2000", (BaseType::Long, _) => "3000",
            (BaseType::Float, 0) => "1.5", (BaseType::Float, 1) => "2.5", (BaseType::Float, _) => "3.5",
            (BaseType::Double, 0) => "3.14", (BaseType::Double, 1) => "6.28", (BaseType::Double, _) => "9.42",
            (BaseType::Str, 0) => "abc", (BaseType::Str, 1) => "def", (BaseType::Str, _) => "ghi",
            (BaseType::Bool, 0) => "true", (BaseType::Bool, _) => "false",
            _ => "?",
        }
    }

    pub fn example_key_n(&self, idx: usize) -> &'static str {
        match (self, idx % 2) {
            (BaseType::Int, 0) => "1", (BaseType::Int, _) => "2",
            (BaseType::Long, 0) => "1000", (BaseType::Long, _) => "2000",
            (BaseType::Float, 0) => "1.0", (BaseType::Float, _) => "2.0",
            (BaseType::Double, 0) => "1.0", (BaseType::Double, _) => "2.0",
            (BaseType::Str, 0) => "hp", (BaseType::Str, _) => "mp",
            _ => "?",
        }
    }

    pub fn rand_value(&self) -> String {
        use std::time::SystemTime;
        let seed = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().subsec_nanos();
        match self {
            BaseType::Int => format!("{}", (seed % 100) as i32 + 1),
            BaseType::Long => format!("{}", (seed % 10000) as i64 + 1000),
            BaseType::Float => format!("{:.1}", (seed % 100) as f32 / 10.0),
            BaseType::Double => format!("{:.2}", (seed % 1000) as f64 / 100.0),
            BaseType::Str => {
                let words = ["fire", "ice", "wind", "earth", "light", "dark"];
                words[(seed as usize) % words.len()].to_string()
            }
            BaseType::Bool => if seed % 2 == 0 { "true".to_string() } else { "false".to_string() },
        }
    }

    pub fn rand_key(&self) -> String {
        use std::time::SystemTime;
        let seed = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().subsec_nanos();
        match self {
            BaseType::Int => format!("{}", (seed % 50) as i32 + 1),
            BaseType::Long => format!("{}", (seed % 5000) as i64 + 1000),
            BaseType::Float => format!("{:.1}", (seed % 20) as f32 + 1.0),
            BaseType::Double => format!("{:.1}", (seed % 20) as f64 + 1.0),
            BaseType::Str => {
                let keys = ["hp", "mp", "atk", "def", "spd", "crit"];
                keys[(seed as usize) % keys.len()].to_string()
            }
            _ => "?".to_string(),
        }
    }
}

// --- Paradigm ---

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Paradigm {
    Base,
    Tuple2,
    Tuple3,
    Tuple4,
    List,
    Set,
    Map,
    ListTuple2,
    ListTuple3,
    ListTuple4,
    MapTuple2,
    MapTuple3,
    MapTuple4,
    MapList,
}

impl Paradigm {
    pub fn all() -> &'static [Paradigm] {
        use Paradigm::*;
        &[Base, Tuple2, Tuple3, Tuple4, List, Set, Map,
          ListTuple2, ListTuple3, ListTuple4,
          MapTuple2, MapTuple3, MapTuple4, MapList]
    }

    pub fn label(&self) -> &'static str {
        match self {
            Paradigm::Base => "基础类型",
            Paradigm::Tuple2 => "Tuple2<_,_>",
            Paradigm::Tuple3 => "Tuple3<_,_,_>",
            Paradigm::Tuple4 => "Tuple4<_,_,_,_>",
            Paradigm::List => "List<_>",
            Paradigm::Set => "Set<_>",
            Paradigm::Map => "Map<_,_>",
            Paradigm::ListTuple2 => "List<Tuple2<_,_>>",
            Paradigm::ListTuple3 => "List<Tuple3<_,_,_>>",
            Paradigm::ListTuple4 => "List<Tuple4<_,_,_,_>>",
            Paradigm::MapTuple2 => "Map<_,Tuple2<_,_>>",
            Paradigm::MapTuple3 => "Map<_,Tuple3<_,_,_>>",
            Paradigm::MapTuple4 => "Map<_,Tuple4<_,_,_,_>>",
            Paradigm::MapList => "Map<_,List<_>>",
        }
    }

    pub fn param_slots(&self) -> Vec<ParamSlot> {
        match self {
            Paradigm::Base => vec![ParamSlot::new("类型", false)],
            Paradigm::Tuple2 => vec![ParamSlot::new("P1", false), ParamSlot::new("P2", false)],
            Paradigm::Tuple3 => vec![ParamSlot::new("P1", false), ParamSlot::new("P2", false), ParamSlot::new("P3", false)],
            Paradigm::Tuple4 => vec![ParamSlot::new("P1", false), ParamSlot::new("P2", false), ParamSlot::new("P3", false), ParamSlot::new("P4", false)],
            Paradigm::List | Paradigm::Set => vec![ParamSlot::new("元素", false)],
            Paradigm::Map => vec![ParamSlot::new("K", true), ParamSlot::new("V", false)],
            Paradigm::ListTuple2 => vec![ParamSlot::new("P1", false), ParamSlot::new("P2", false)],
            Paradigm::ListTuple3 => vec![ParamSlot::new("P1", false), ParamSlot::new("P2", false), ParamSlot::new("P3", false)],
            Paradigm::ListTuple4 => vec![ParamSlot::new("P1", false), ParamSlot::new("P2", false), ParamSlot::new("P3", false), ParamSlot::new("P4", false)],
            Paradigm::MapTuple2 => vec![ParamSlot::new("K", true), ParamSlot::new("P1", false), ParamSlot::new("P2", false)],
            Paradigm::MapTuple3 => vec![ParamSlot::new("K", true), ParamSlot::new("P1", false), ParamSlot::new("P2", false), ParamSlot::new("P3", false)],
            Paradigm::MapTuple4 => vec![ParamSlot::new("K", true), ParamSlot::new("P1", false), ParamSlot::new("P2", false), ParamSlot::new("P3", false), ParamSlot::new("P4", false)],
            Paradigm::MapList => vec![ParamSlot::new("K", true), ParamSlot::new("元素", false)],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ParamSlot {
    pub label: &'static str,
    pub is_map_key: bool,
}

impl ParamSlot {
    fn new(label: &'static str, is_map_key: bool) -> Self {
        Self { label, is_map_key }
    }
}

// --- TblType: a fully resolved type ---

#[derive(Clone, Debug, PartialEq)]
pub struct TblType {
    pub paradigm: Paradigm,
    pub params: Vec<BaseType>,
}

impl TblType {
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim();
        if let Some(bt) = BaseType::from_str(s) {
            return Some(TblType { paradigm: Paradigm::Base, params: vec![bt] });
        }
        if let Some(inner) = strip_wrapper(s, "Tuple2<", ">") {
            let params = parse_base_params(inner)?;
            if params.len() == 2 { return Some(TblType { paradigm: Paradigm::Tuple2, params }); }
        }
        if let Some(inner) = strip_wrapper(s, "Tuple3<", ">") {
            let params = parse_base_params(inner)?;
            if params.len() == 3 { return Some(TblType { paradigm: Paradigm::Tuple3, params }); }
        }
        if let Some(inner) = strip_wrapper(s, "Tuple4<", ">") {
            let params = parse_base_params(inner)?;
            if params.len() == 4 { return Some(TblType { paradigm: Paradigm::Tuple4, params }); }
        }
        if let Some(inner) = strip_wrapper(s, "Set<", ">") {
            let bt = BaseType::from_str(inner)?;
            return Some(TblType { paradigm: Paradigm::Set, params: vec![bt] });
        }
        if let Some(inner) = strip_wrapper(s, "List<Tuple2<", ">>") {
            let params = parse_base_params(inner)?;
            if params.len() == 2 { return Some(TblType { paradigm: Paradigm::ListTuple2, params }); }
        }
        if let Some(inner) = strip_wrapper(s, "List<Tuple3<", ">>") {
            let params = parse_base_params(inner)?;
            if params.len() == 3 { return Some(TblType { paradigm: Paradigm::ListTuple3, params }); }
        }
        if let Some(inner) = strip_wrapper(s, "List<Tuple4<", ">>") {
            let params = parse_base_params(inner)?;
            if params.len() == 4 { return Some(TblType { paradigm: Paradigm::ListTuple4, params }); }
        }
        if let Some(inner) = strip_wrapper(s, "List<", ">") {
            let bt = BaseType::from_str(inner)?;
            return Some(TblType { paradigm: Paradigm::List, params: vec![bt] });
        }
        if let Some(inner) = strip_wrapper(s, "Map<", ">") {
            let raw_parts = split_params(inner);
            if raw_parts.len() >= 2 {
                let key = BaseType::from_str(&raw_parts[0])?;
                let val_str = raw_parts[1..].join(",");
                if let Some(t_inner) = strip_wrapper(&val_str, "Tuple2<", ">") {
                    let mut params = vec![key];
                    params.extend(parse_base_params(t_inner)?);
                    if params.len() == 3 { return Some(TblType { paradigm: Paradigm::MapTuple2, params }); }
                }
                if let Some(t_inner) = strip_wrapper(&val_str, "Tuple3<", ">") {
                    let mut params = vec![key];
                    params.extend(parse_base_params(t_inner)?);
                    if params.len() == 4 { return Some(TblType { paradigm: Paradigm::MapTuple3, params }); }
                }
                if let Some(t_inner) = strip_wrapper(&val_str, "Tuple4<", ">") {
                    let mut params = vec![key];
                    params.extend(parse_base_params(t_inner)?);
                    if params.len() == 5 { return Some(TblType { paradigm: Paradigm::MapTuple4, params }); }
                }
                if let Some(l_inner) = strip_wrapper(&val_str, "List<", ">") {
                    let elem = BaseType::from_str(l_inner)?;
                    return Some(TblType { paradigm: Paradigm::MapList, params: vec![key, elem] });
                }
                if raw_parts.len() == 2 {
                    let val = BaseType::from_str(&raw_parts[1])?;
                    return Some(TblType { paradigm: Paradigm::Map, params: vec![key, val] });
                }
            }
        }
        None
    }

    pub fn to_type_string(&self) -> String {
        let p: Vec<&str> = self.params.iter().map(|b| b.name()).collect();
        match &self.paradigm {
            Paradigm::Base => p[0].to_string(),
            Paradigm::Tuple2 => format!("Tuple2<{},{}>", p[0], p[1]),
            Paradigm::Tuple3 => format!("Tuple3<{},{},{}>", p[0], p[1], p[2]),
            Paradigm::Tuple4 => format!("Tuple4<{},{},{},{}>", p[0], p[1], p[2], p[3]),
            Paradigm::List => format!("List<{}>", p[0]),
            Paradigm::Set => format!("Set<{}>", p[0]),
            Paradigm::Map => format!("Map<{},{}>", p[0], p[1]),
            Paradigm::ListTuple2 => format!("List<Tuple2<{},{}>>", p[0], p[1]),
            Paradigm::ListTuple3 => format!("List<Tuple3<{},{},{}>>", p[0], p[1], p[2]),
            Paradigm::ListTuple4 => format!("List<Tuple4<{},{},{},{}>>", p[0], p[1], p[2], p[3]),
            Paradigm::MapTuple2 => format!("Map<{},Tuple2<{},{}>>", p[0], p[1], p[2]),
            Paradigm::MapTuple3 => format!("Map<{},Tuple3<{},{},{}>>", p[0], p[1], p[2], p[3]),
            Paradigm::MapTuple4 => format!("Map<{},Tuple4<{},{},{},{}>>", p[0], p[1], p[2], p[3], p[4]),
            Paradigm::MapList => format!("Map<{},List<{}>>", p[0], p[1]),
        }
    }

    pub fn java_decl(&self) -> String {
        let p = &self.params;
        match &self.paradigm {
            Paradigm::Base => p[0].java_type().to_string(),
            Paradigm::Tuple2 => format!("Tuple2<{},{}>", p[0].java_boxed(), p[1].java_boxed()),
            Paradigm::Tuple3 => format!("Tuple3<{},{},{}>", p[0].java_boxed(), p[1].java_boxed(), p[2].java_boxed()),
            Paradigm::Tuple4 => format!("Tuple4<{},{},{},{}>", p[0].java_boxed(), p[1].java_boxed(), p[2].java_boxed(), p[3].java_boxed()),
            Paradigm::List => format!("List<{}>", p[0].java_boxed()),
            Paradigm::Set => format!("Set<{}>", p[0].java_boxed()),
            Paradigm::Map => format!("Map<{},{}>", p[0].java_boxed(), p[1].java_boxed()),
            Paradigm::ListTuple2 => format!("List<Tuple2<{},{}>>", p[0].java_boxed(), p[1].java_boxed()),
            Paradigm::ListTuple3 => format!("List<Tuple3<{},{},{}>>", p[0].java_boxed(), p[1].java_boxed(), p[2].java_boxed()),
            Paradigm::ListTuple4 => format!("List<Tuple4<{},{},{},{}>>", p[0].java_boxed(), p[1].java_boxed(), p[2].java_boxed(), p[3].java_boxed()),
            Paradigm::MapTuple2 => format!("Map<{},Tuple2<{},{}>>", p[0].java_boxed(), p[1].java_boxed(), p[2].java_boxed()),
            Paradigm::MapTuple3 => format!("Map<{},Tuple3<{},{},{}>>", p[0].java_boxed(), p[1].java_boxed(), p[2].java_boxed(), p[3].java_boxed()),
            Paradigm::MapTuple4 => format!("Map<{},Tuple4<{},{},{},{}>>", p[0].java_boxed(), p[1].java_boxed(), p[2].java_boxed(), p[3].java_boxed(), p[4].java_boxed()),
            Paradigm::MapList => format!("Map<{},List<{}>>", p[0].java_boxed(), p[1].java_boxed()),
        }
    }

    pub fn go_decl(&self) -> String {
        let p = &self.params;
        match &self.paradigm {
            Paradigm::Base => p[0].go_type().to_string(),
            Paradigm::Tuple2 => format!("[2]{}", p[0].go_type()),
            Paradigm::Tuple3 => format!("[3]{}", p[0].go_type()),
            Paradigm::Tuple4 => format!("[4]{}", p[0].go_type()),
            Paradigm::List => format!("[]{}", p[0].go_type()),
            Paradigm::Set => format!("map[{}]struct{{}}", p[0].go_type()),
            Paradigm::Map => format!("map[{}]{}", p[0].go_type(), p[1].go_type()),
            Paradigm::ListTuple2 => format!("[][2]{}", p[0].go_type()),
            Paradigm::ListTuple3 => format!("[][3]{}", p[0].go_type()),
            Paradigm::ListTuple4 => format!("[][4]{}", p[0].go_type()),
            Paradigm::MapTuple2 => format!("map[{}][2]{}", p[0].go_type(), p[1].go_type()),
            Paradigm::MapTuple3 => format!("map[{}][3]{}", p[0].go_type(), p[1].go_type()),
            Paradigm::MapTuple4 => format!("map[{}][4]{}", p[0].go_type(), p[1].go_type()),
            Paradigm::MapList => format!("map[{}][]{}", p[0].go_type(), p[1].go_type()),
        }
    }

    pub fn lua_decl(&self) -> String {
        let p = &self.params;
        match &self.paradigm {
            Paradigm::Base => p[0].lua_type().to_string(),
            Paradigm::Tuple2 | Paradigm::Tuple3 | Paradigm::Tuple4 => "{p1, p2, ...}".to_string(),
            Paradigm::List => format!("{{{}, ...}}", p[0].lua_type()),
            Paradigm::Set => "{[v]=true, ...}".to_string(),
            Paradigm::Map => format!("{{k={}, ...}}", p[1].lua_type()),
            Paradigm::ListTuple2 | Paradigm::ListTuple3 | Paradigm::ListTuple4 => "{{...}, {...}}".to_string(),
            Paradigm::MapTuple2 | Paradigm::MapTuple3 | Paradigm::MapTuple4 => "{k={...}, ...}".to_string(),
            Paradigm::MapList => format!("{{k={{{}, ...}}}}", p[1].lua_type()),
        }
    }

    pub fn example(&self) -> String {
        let sep = SeparatorsSection::default();
        self.example_with_sep(&sep)
    }

    pub fn example_with_sep(&self, sep: &SeparatorsSection) -> String {
        self.build_demo(sep, false)
    }

    pub fn random_demo(&self, sep: &SeparatorsSection) -> String {
        self.build_demo(sep, true)
    }

    fn build_demo(&self, sep: &SeparatorsSection, random: bool) -> String {
        let p = &self.params;
        let v = |bt: BaseType, idx: usize| -> String { if random { bt.rand_value() } else { bt.example_n(idx).to_string() } };
        let k = |bt: BaseType, idx: usize| -> String { if random { bt.rand_key() } else { bt.example_key_n(idx).to_string() } };
        match &self.paradigm {
            Paradigm::Base => v(p[0], 0),
            Paradigm::Tuple2 => format!("{}{}{}", v(p[0], 0), sep.tuple2, v(p[1], 0)),
            Paradigm::Tuple3 => format!("{}{}{}{}{}", v(p[0], 0), sep.tuple3, v(p[1], 0), sep.tuple3, v(p[2], 0)),
            Paradigm::Tuple4 => format!("{}{}{}{}{}{}{}", v(p[0], 0), sep.tuple4, v(p[1], 0), sep.tuple4, v(p[2], 0), sep.tuple4, v(p[3], 0)),
            Paradigm::List => format!("{}{}{}{}{}", v(p[0], 0), sep.list, v(p[0], 1), sep.list, v(p[0], 2)),
            Paradigm::Set => format!("{}{}{}{}{}", v(p[0], 0), sep.set, v(p[0], 1), sep.set, v(p[0], 2)),
            Paradigm::Map => format!("{}{}{}{}{}{}{}",
                k(p[0], 0), sep.map.kv, v(p[1], 0), sep.map.entry,
                k(p[0], 1), sep.map.kv, v(p[1], 1)),
            Paradigm::ListTuple2 => format!("{}{}{}{}{}{}{}",
                v(p[0], 0), sep.list_tuple2.tuple, v(p[1], 0), sep.list_tuple2.list,
                v(p[0], 1), sep.list_tuple2.tuple, v(p[1], 1)),
            Paradigm::ListTuple3 => format!("{}{}{}{}{}{}{}{}{}{}{}",
                v(p[0], 0), sep.list_tuple3.tuple, v(p[1], 0), sep.list_tuple3.tuple, v(p[2], 0), sep.list_tuple3.list,
                v(p[0], 1), sep.list_tuple3.tuple, v(p[1], 1), sep.list_tuple3.tuple, v(p[2], 1)),
            Paradigm::ListTuple4 => format!("{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
                v(p[0], 0), sep.list_tuple4.tuple, v(p[1], 0), sep.list_tuple4.tuple, v(p[2], 0), sep.list_tuple4.tuple, v(p[3], 0), sep.list_tuple4.list,
                v(p[0], 1), sep.list_tuple4.tuple, v(p[1], 1), sep.list_tuple4.tuple, v(p[2], 1), sep.list_tuple4.tuple, v(p[3], 1)),
            Paradigm::MapTuple2 => format!("{}{}{}{}{}{}{}{}{}{}{}",
                k(p[0], 0), sep.map_tuple2.kv, v(p[1], 0), sep.map_tuple2.tuple, v(p[2], 0), sep.map_tuple2.entry,
                k(p[0], 1), sep.map_tuple2.kv, v(p[1], 1), sep.map_tuple2.tuple, v(p[2], 1)),
            Paradigm::MapTuple3 => format!("{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
                k(p[0], 0), sep.map_tuple3.kv, v(p[1], 0), sep.map_tuple3.tuple, v(p[2], 0), sep.map_tuple3.tuple, v(p[3], 0), sep.map_tuple3.entry,
                k(p[0], 1), sep.map_tuple3.kv, v(p[1], 1), sep.map_tuple3.tuple, v(p[2], 1), sep.map_tuple3.tuple, v(p[3], 1)),
            Paradigm::MapTuple4 => format!("{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}{}",
                k(p[0], 0), sep.map_tuple4.kv, v(p[1], 0), sep.map_tuple4.tuple, v(p[2], 0), sep.map_tuple4.tuple, v(p[3], 0), sep.map_tuple4.tuple, v(p[3], 1), sep.map_tuple4.entry,
                k(p[0], 1), sep.map_tuple4.kv, v(p[1], 1), sep.map_tuple4.tuple, v(p[2], 1), sep.map_tuple4.tuple, v(p[3], 1), sep.map_tuple4.tuple, v(p[3], 2)),
            Paradigm::MapList => format!("{}{}{}{}{}{}{}{}{}{}{}",
                k(p[0], 0), sep.map_list.kv, v(p[1], 0), sep.map_list.item, v(p[1], 1), sep.map_list.item, v(p[1], 2), sep.map_list.entry,
                k(p[0], 1), sep.map_list.kv, v(p[1], 0)),
        }
    }

    /// Validate a cell value against this type. Returns None if valid, Some(error_msg) if invalid.
    pub fn validate_value(&self, value: &str, sep: &SeparatorsSection) -> Option<String> {
        if value.is_empty() { return None; }
        if let Some(msg) = check_chinese_punctuation(value) { return Some(msg.to_string()); }
        let p = &self.params;
        let err = match &self.paradigm {
            Paradigm::Base => validate_base(value, p[0]),
            Paradigm::Tuple2 => validate_tuple(value, &p[0..2], &sep.tuple2),
            Paradigm::Tuple3 => validate_tuple(value, &p[0..3], &sep.tuple3),
            Paradigm::Tuple4 => validate_tuple(value, &p[0..4], &sep.tuple4),
            Paradigm::List => validate_list(value, p[0], &sep.list),
            Paradigm::Set => validate_list(value, p[0], &sep.set),
            Paradigm::Map => validate_map(value, p[0], p[1], &sep.map.entry, &sep.map.kv),
            Paradigm::ListTuple2 => validate_list_tuple(value, &p[0..2], &sep.list_tuple2.list, &sep.list_tuple2.tuple),
            Paradigm::ListTuple3 => validate_list_tuple(value, &p[0..3], &sep.list_tuple3.list, &sep.list_tuple3.tuple),
            Paradigm::ListTuple4 => validate_list_tuple(value, &p[0..4], &sep.list_tuple4.list, &sep.list_tuple4.tuple),
            Paradigm::MapTuple2 => validate_map_tuple(value, p[0], &p[1..3], &sep.map_tuple2.entry, &sep.map_tuple2.kv, &sep.map_tuple2.tuple),
            Paradigm::MapTuple3 => validate_map_tuple(value, p[0], &p[1..4], &sep.map_tuple3.entry, &sep.map_tuple3.kv, &sep.map_tuple3.tuple),
            Paradigm::MapTuple4 => validate_map_tuple(value, p[0], &p[1..5], &sep.map_tuple4.entry, &sep.map_tuple4.kv, &sep.map_tuple4.tuple),
            Paradigm::MapList => validate_map_list(value, p[0], p[1], &sep.map_list.entry, &sep.map_list.kv, &sep.map_list.item),
        };
        err.map(|msg| format!("{}, 示例: {}", msg, self.example_with_sep(sep)))
    }
}

fn check_chinese_punctuation(value: &str) -> Option<&'static str> {
    for c in value.chars() {
        match c {
            '，' | '；' | '：' | '、' => return Some("含有中文标点符号"),
            _ => {}
        }
    }
    None
}

fn validate_base(value: &str, bt: BaseType) -> Option<&'static str> {
    match bt {
        BaseType::Int => { if value.parse::<i32>().is_err() { Some("不是合法int") } else { None } }
        BaseType::Long => { if value.parse::<i64>().is_err() { Some("不是合法long") } else { None } }
        BaseType::Float => { if value.parse::<f32>().is_err() { Some("不是合法float") } else { None } }
        BaseType::Double => { if value.parse::<f64>().is_err() { Some("不是合法double") } else { None } }
        BaseType::Bool => { if value != "true" && value != "false" { Some("必须是true或false") } else { None } }
        BaseType::Str => None,
    }
}

fn validate_tuple(value: &str, types: &[BaseType], sep: &str) -> Option<&'static str> {
    let parts: Vec<&str> = value.split(sep).collect();
    if parts.len() != types.len() { return Some("元素数量不匹配"); }
    for (part, &bt) in parts.iter().zip(types.iter()) {
        let p = part.trim();
        if p.is_empty() { return Some("含有空元素"); }
        if validate_base(p, bt).is_some() { return Some("元素类型不匹配"); }
    }
    None
}

fn validate_list(value: &str, elem: BaseType, sep: &str) -> Option<&'static str> {
    for part in value.split(sep) {
        let p = part.trim();
        if p.is_empty() { return Some("含有空元素"); }
        if validate_base(p, elem).is_some() { return Some("列表元素类型不匹配"); }
    }
    None
}

fn validate_map(value: &str, key: BaseType, val: BaseType, entry_sep: &str, kv_sep: &str) -> Option<&'static str> {
    for entry in value.split(entry_sep) {
        let entry = entry.trim();
        if entry.is_empty() { return Some("含有空条目"); }
        let kv: Vec<&str> = entry.splitn(2, kv_sep).collect();
        if kv.len() != 2 { return Some("缺少kv分隔符"); }
        let k = kv[0].trim();
        let v = kv[1].trim();
        if k.is_empty() { return Some("key为空"); }
        if v.is_empty() { return Some("value为空"); }
        if validate_base(k, key).is_some() { return Some("key类型不匹配"); }
        if validate_base(v, val).is_some() { return Some("value类型不匹配"); }
    }
    None
}

fn validate_list_tuple(value: &str, types: &[BaseType], list_sep: &str, tuple_sep: &str) -> Option<&'static str> {
    for item in value.split(list_sep) {
        let item = item.trim();
        if item.is_empty() { return Some("含有空元素"); }
        if validate_tuple(item, types, tuple_sep).is_some() { return Some("列表元素格式错误"); }
    }
    None
}

fn validate_map_tuple(value: &str, key: BaseType, val_types: &[BaseType], entry_sep: &str, kv_sep: &str, tuple_sep: &str) -> Option<&'static str> {
    for entry in value.split(entry_sep) {
        let entry = entry.trim();
        if entry.is_empty() { return Some("含有空条目"); }
        let kv: Vec<&str> = entry.splitn(2, kv_sep).collect();
        if kv.len() != 2 { return Some("缺少kv分隔符"); }
        let k = kv[0].trim();
        if k.is_empty() { return Some("key为空"); }
        if validate_base(k, key).is_some() { return Some("key类型不匹配"); }
        if validate_tuple(kv[1].trim(), val_types, tuple_sep).is_some() { return Some("value格式错误"); }
    }
    None
}

fn validate_map_list(value: &str, key: BaseType, elem: BaseType, entry_sep: &str, kv_sep: &str, item_sep: &str) -> Option<&'static str> {
    for entry in value.split(entry_sep) {
        let entry = entry.trim();
        if entry.is_empty() { return Some("含有空条目"); }
        let kv: Vec<&str> = entry.splitn(2, kv_sep).collect();
        if kv.len() != 2 { return Some("缺少kv分隔符"); }
        let k = kv[0].trim();
        if k.is_empty() { return Some("key为空"); }
        if validate_base(k, key).is_some() { return Some("key类型不匹配"); }
        for item in kv[1].split(item_sep) {
            let item = item.trim();
            if item.is_empty() { return Some("含有空元素"); }
            if validate_base(item, elem).is_some() { return Some("value列表元素类型不匹配"); }
        }
    }
    None
}

// --- Separator Config ---

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct SeparatorsSection {
    #[serde(default = "default_comma")]
    pub tuple2: String,
    #[serde(default = "default_comma")]
    pub tuple3: String,
    #[serde(default = "default_comma")]
    pub tuple4: String,
    #[serde(default = "default_semicolon")]
    pub list: String,
    #[serde(default = "default_semicolon")]
    pub set: String,
    #[serde(default)]
    pub map: MapSep,
    #[serde(default, rename = "List_Tuple2")]
    pub list_tuple2: ListTupleSep,
    #[serde(default, rename = "List_Tuple3")]
    pub list_tuple3: ListTupleSep,
    #[serde(default, rename = "List_Tuple4")]
    pub list_tuple4: ListTupleSep,
    #[serde(default, rename = "Map_Tuple2")]
    pub map_tuple2: MapTupleSep,
    #[serde(default, rename = "Map_Tuple3")]
    pub map_tuple3: MapTupleSep,
    #[serde(default, rename = "Map_Tuple4")]
    pub map_tuple4: MapTupleSep,
    #[serde(default, rename = "Map_List")]
    pub map_list: MapListSep,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MapSep {
    #[serde(default = "default_colon")]
    pub kv: String,
    #[serde(default = "default_semicolon")]
    pub entry: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ListTupleSep {
    #[serde(default = "default_comma")]
    pub tuple: String,
    #[serde(default = "default_semicolon")]
    pub list: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MapTupleSep {
    #[serde(default = "default_colon")]
    pub kv: String,
    #[serde(default = "default_comma")]
    pub tuple: String,
    #[serde(default = "default_semicolon")]
    pub entry: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct MapListSep {
    #[serde(default = "default_colon")]
    pub kv: String,
    #[serde(default = "default_comma")]
    pub item: String,
    #[serde(default = "default_semicolon")]
    pub entry: String,
}

fn default_comma() -> String { ",".to_string() }
fn default_semicolon() -> String { ";".to_string() }
fn default_colon() -> String { ":".to_string() }

impl Default for MapSep { fn default() -> Self { Self { kv: default_colon(), entry: default_semicolon() } } }
impl Default for ListTupleSep { fn default() -> Self { Self { tuple: default_comma(), list: default_semicolon() } } }
impl Default for MapTupleSep { fn default() -> Self { Self { kv: default_colon(), tuple: default_comma(), entry: default_semicolon() } } }
impl Default for MapListSep { fn default() -> Self { Self { kv: default_colon(), item: default_comma(), entry: default_semicolon() } } }

impl Default for SeparatorsSection {
    fn default() -> Self {
        Self {
            tuple2: default_comma(), tuple3: default_comma(), tuple4: default_comma(),
            list: default_semicolon(), set: default_semicolon(),
            map: MapSep::default(),
            list_tuple2: ListTupleSep::default(), list_tuple3: ListTupleSep::default(), list_tuple4: ListTupleSep::default(),
            map_tuple2: MapTupleSep::default(), map_tuple3: MapTupleSep::default(), map_tuple4: MapTupleSep::default(),
            map_list: MapListSep::default(),
        }
    }
}

fn strip_wrapper<'a>(s: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)?.strip_suffix(suffix)
}

fn split_params(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut depth = 0;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match c {
            '<' => depth += 1,
            '>' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(s[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(s[start..].trim().to_string());
    parts
}

fn parse_base_params(s: &str) -> Option<Vec<BaseType>> {
    split_params(s).iter().map(|p| BaseType::from_str(p.trim())).collect()
}
