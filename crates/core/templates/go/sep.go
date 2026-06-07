package {{PACKAGE}}

// SepConfig 包含项目所有分隔符配置，跟随每个数据文件加载。
// 字段命名为 snake_case 与数据文件保持一致。
type SepConfig struct {
	List string
	Set  string

	Tuple2 string
	Tuple3 string
	Tuple4 string

	MapKv    string
	MapEntry string

	ListTuple2Tuple string
	ListTuple2List  string
	ListTuple3Tuple string
	ListTuple3List  string
	ListTuple4Tuple string
	ListTuple4List  string

	MapTuple2Kv    string
	MapTuple2Tuple string
	MapTuple2Entry string
	MapTuple3Kv    string
	MapTuple3Tuple string
	MapTuple3Entry string
	MapTuple4Kv    string
	MapTuple4Tuple string
	MapTuple4Entry string

	MapListKv    string
	MapListItem  string
	MapListEntry string
}

// 缺失字段时回退到默认分隔符（与 Rust SeparatorsSection::default() 一致）
func (s *SepConfig) fillDefaults() {
	defaultStr(&s.List, ";")
	defaultStr(&s.Set, ";")
	defaultStr(&s.Tuple2, ",")
	defaultStr(&s.Tuple3, ",")
	defaultStr(&s.Tuple4, ",")
	defaultStr(&s.MapKv, ":")
	defaultStr(&s.MapEntry, ";")
	defaultStr(&s.ListTuple2Tuple, ",")
	defaultStr(&s.ListTuple2List, ";")
	defaultStr(&s.ListTuple3Tuple, ",")
	defaultStr(&s.ListTuple3List, ";")
	defaultStr(&s.ListTuple4Tuple, ",")
	defaultStr(&s.ListTuple4List, ";")
	defaultStr(&s.MapTuple2Kv, ":")
	defaultStr(&s.MapTuple2Tuple, ",")
	defaultStr(&s.MapTuple2Entry, ";")
	defaultStr(&s.MapTuple3Kv, ":")
	defaultStr(&s.MapTuple3Tuple, ",")
	defaultStr(&s.MapTuple3Entry, ";")
	defaultStr(&s.MapTuple4Kv, ":")
	defaultStr(&s.MapTuple4Tuple, ",")
	defaultStr(&s.MapTuple4Entry, ";")
	defaultStr(&s.MapListKv, ":")
	defaultStr(&s.MapListItem, ",")
	defaultStr(&s.MapListEntry, ";")
}

func defaultStr(target *string, fallback string) {
	if *target == "" {
		*target = fallback
	}
}

// 从 JSON 数据文件中的 _sep 对象构建 SepConfig
func sepFromJSONMap(m map[string]string) SepConfig {
	cfg := SepConfig{
		List:            m["list"],
		Set:             m["set"],
		Tuple2:          m["tuple2"],
		Tuple3:          m["tuple3"],
		Tuple4:          m["tuple4"],
		MapKv:           m["map_kv"],
		MapEntry:        m["map_entry"],
		ListTuple2Tuple: m["list_tuple2_tuple"],
		ListTuple2List:  m["list_tuple2_list"],
		ListTuple3Tuple: m["list_tuple3_tuple"],
		ListTuple3List:  m["list_tuple3_list"],
		ListTuple4Tuple: m["list_tuple4_tuple"],
		ListTuple4List:  m["list_tuple4_list"],
		MapTuple2Kv:     m["map_tuple2_kv"],
		MapTuple2Tuple:  m["map_tuple2_tuple"],
		MapTuple2Entry:  m["map_tuple2_entry"],
		MapTuple3Kv:     m["map_tuple3_kv"],
		MapTuple3Tuple:  m["map_tuple3_tuple"],
		MapTuple3Entry:  m["map_tuple3_entry"],
		MapTuple4Kv:     m["map_tuple4_kv"],
		MapTuple4Tuple:  m["map_tuple4_tuple"],
		MapTuple4Entry:  m["map_tuple4_entry"],
		MapListKv:       m["map_list_kv"],
		MapListItem:     m["map_list_item"],
		MapListEntry:    m["map_list_entry"],
	}
	cfg.fillDefaults()
	return cfg
}

// 从 XML 根元素属性构建 SepConfig（attr 名以 sep_ 开头）
func sepFromXMLAttrs(attrs map[string]string) SepConfig {
	cfg := SepConfig{
		List:     attrs["sep_list"],
		Set:      attrs["sep_set"],
		Tuple2:   attrs["sep_tuple2"],
		Tuple3:   attrs["sep_tuple3"],
		Tuple4:   attrs["sep_tuple4"],
		MapKv:    attrs["sep_map_kv"],
		MapEntry: attrs["sep_map_entry"],
	}
	cfg.fillDefaults()
	return cfg
}
