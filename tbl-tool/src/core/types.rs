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

    pub fn example(&self) -> &'static str {
        match self {
            BaseType::Int => "1",
            BaseType::Long => "1000",
            BaseType::Float => "1.5",
            BaseType::Double => "3.14",
            BaseType::Str => "abc",
            BaseType::Bool => "true",
        }
    }

    pub fn example2(&self) -> &'static str {
        match self {
            BaseType::Int => "2",
            BaseType::Long => "2000",
            BaseType::Float => "2.5",
            BaseType::Double => "6.28",
            BaseType::Str => "def",
            BaseType::Bool => "false",
        }
    }

    pub fn example3(&self) -> &'static str {
        match self {
            BaseType::Int => "3",
            BaseType::Long => "3000",
            BaseType::Float => "3.5",
            BaseType::Double => "9.42",
            BaseType::Str => "ghi",
            BaseType::Bool => "true",
        }
    }

    pub fn example_key(&self) -> &'static str {
        match self {
            BaseType::Int => "1",
            BaseType::Long => "1000",
            BaseType::Float => "1.0",
            BaseType::Double => "1.0",
            BaseType::Str => "hp",
            _ => "?",
        }
    }

    pub fn example_key2(&self) -> &'static str {
        match self {
            BaseType::Int => "2",
            BaseType::Long => "2000",
            BaseType::Float => "2.0",
            BaseType::Double => "2.0",
            BaseType::Str => "mp",
            _ => "?",
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
        let p = &self.params;
        match &self.paradigm {
            Paradigm::Base => p[0].example().to_string(),
            Paradigm::Tuple2 => format!("{},{}", p[0].example(), p[1].example()),
            Paradigm::Tuple3 => format!("{},{},{}", p[0].example(), p[1].example(), p[2].example()),
            Paradigm::Tuple4 => format!("{},{},{},{}", p[0].example(), p[1].example(), p[2].example(), p[3].example()),
            Paradigm::List => format!("{};{};{}", p[0].example(), p[0].example2(), p[0].example3()),
            Paradigm::Set => format!("{};{};{}", p[0].example(), p[0].example2(), p[0].example3()),
            Paradigm::Map => format!("{}:{};{}:{}", p[0].example_key(), p[1].example(), p[0].example_key2(), p[1].example2()),
            Paradigm::ListTuple2 => format!("{},{};{},{}", p[0].example(), p[1].example(), p[0].example2(), p[1].example2()),
            Paradigm::ListTuple3 => format!("{},{},{};{},{},{}", p[0].example(), p[1].example(), p[2].example(), p[0].example2(), p[1].example2(), p[2].example2()),
            Paradigm::ListTuple4 => format!("{},{},{},{};{},{},{},{}", p[0].example(), p[1].example(), p[2].example(), p[3].example(), p[0].example2(), p[1].example2(), p[2].example2(), p[3].example2()),
            Paradigm::MapTuple2 => format!("{}:{},{};{}:{},{}", p[0].example_key(), p[1].example(), p[2].example(), p[0].example_key2(), p[1].example2(), p[2].example2()),
            Paradigm::MapTuple3 => format!("{}:{},{},{};{}:{},{},{}", p[0].example_key(), p[1].example(), p[2].example(), p[3].example(), p[0].example_key2(), p[1].example2(), p[2].example2(), p[3].example2()),
            Paradigm::MapTuple4 => format!("{}:{},{},{},{};{}:{},{},{},{}", p[0].example_key(), p[1].example(), p[2].example(), p[3].example(), p[3].example2(), p[0].example_key2(), p[1].example2(), p[2].example2(), p[3].example2(), p[3].example3()),
            Paradigm::MapList => format!("{}:{},{},{};{}:{},{}", p[0].example_key(), p[1].example(), p[1].example2(), p[1].example3(), p[0].example_key2(), p[1].example(), p[1].example2()),
        }
    }
}

// --- Helpers ---

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
