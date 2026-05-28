package {{PACKAGE}};

public class SepConfig {
    public String list = ";";
    public String set = ";";
    public String tuple2 = ",";
    public String tuple3 = ",";
    public String tuple4 = ",";
    public String mapKv = ":";
    public String mapEntry = ";";
    public String listTuple2Tuple = ",";
    public String listTuple2List = ";";
    public String listTuple3Tuple = ",";
    public String listTuple3List = ";";
    public String listTuple4Tuple = ",";
    public String listTuple4List = ";";
    public String mapTuple2Kv = ":";
    public String mapTuple2Tuple = ",";
    public String mapTuple2Entry = ";";
    public String mapTuple3Kv = ":";
    public String mapTuple3Tuple = ",";
    public String mapTuple3Entry = ";";
    public String mapTuple4Kv = ":";
    public String mapTuple4Tuple = ",";
    public String mapTuple4Entry = ";";
    public String mapListKv = ":";
    public String mapListItem = ",";
    public String mapListEntry = ";";

    public static SepConfig fromMap(java.util.Map<String, String> map) {
        SepConfig c = new SepConfig();
        if (map == null || map.isEmpty()) return c;
        c.list = getAny(map, c.list, "list", "sep_list");
        c.set = getAny(map, c.set, "set", "sep_set");
        c.tuple2 = getAny(map, c.tuple2, "tuple2", "sep_tuple2");
        c.tuple3 = getAny(map, c.tuple3, "tuple3", "sep_tuple3");
        c.tuple4 = getAny(map, c.tuple4, "tuple4", "sep_tuple4");
        c.mapKv = getAny(map, c.mapKv, "map_kv", "sep_map_kv");
        c.mapEntry = getAny(map, c.mapEntry, "map_entry", "sep_map_entry");
        c.listTuple2Tuple = getAny(map, c.listTuple2Tuple, "list_tuple2_tuple", "sep_list_tuple2_tuple");
        c.listTuple2List = getAny(map, c.listTuple2List, "list_tuple2_list", "sep_list_tuple2_list");
        c.listTuple3Tuple = getAny(map, c.listTuple3Tuple, "list_tuple3_tuple", "sep_list_tuple3_tuple");
        c.listTuple3List = getAny(map, c.listTuple3List, "list_tuple3_list", "sep_list_tuple3_list");
        c.listTuple4Tuple = getAny(map, c.listTuple4Tuple, "list_tuple4_tuple", "sep_list_tuple4_tuple");
        c.listTuple4List = getAny(map, c.listTuple4List, "list_tuple4_list", "sep_list_tuple4_list");
        c.mapTuple2Kv = getAny(map, c.mapTuple2Kv, "map_tuple2_kv", "sep_map_tuple2_kv");
        c.mapTuple2Tuple = getAny(map, c.mapTuple2Tuple, "map_tuple2_tuple", "sep_map_tuple2_tuple");
        c.mapTuple2Entry = getAny(map, c.mapTuple2Entry, "map_tuple2_entry", "sep_map_tuple2_entry");
        c.mapTuple3Kv = getAny(map, c.mapTuple3Kv, "map_tuple3_kv", "sep_map_tuple3_kv");
        c.mapTuple3Tuple = getAny(map, c.mapTuple3Tuple, "map_tuple3_tuple", "sep_map_tuple3_tuple");
        c.mapTuple3Entry = getAny(map, c.mapTuple3Entry, "map_tuple3_entry", "sep_map_tuple3_entry");
        c.mapTuple4Kv = getAny(map, c.mapTuple4Kv, "map_tuple4_kv", "sep_map_tuple4_kv");
        c.mapTuple4Tuple = getAny(map, c.mapTuple4Tuple, "map_tuple4_tuple", "sep_map_tuple4_tuple");
        c.mapTuple4Entry = getAny(map, c.mapTuple4Entry, "map_tuple4_entry", "sep_map_tuple4_entry");
        c.mapListKv = getAny(map, c.mapListKv, "map_list_kv", "sep_map_list_kv");
        c.mapListItem = getAny(map, c.mapListItem, "map_list_item", "sep_map_list_item");
        c.mapListEntry = getAny(map, c.mapListEntry, "map_list_entry", "sep_map_list_entry");
        return c;
    }

    private static String getAny(java.util.Map<String, String> map, String def, String... keys) {
        for (String k : keys) {
            String v = map.get(k);
            if (v != null) return v;
        }
        return def;
    }
}
