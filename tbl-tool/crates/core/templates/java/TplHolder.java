package {{PACKAGE}};

import java.io.*;
import java.nio.file.*;
import java.util.*;

@SuppressWarnings("unchecked")
public class TplHolder {
    private static final Map<Class<?>, Map<Integer, ? extends ITpl>> tables = new HashMap<>();
    private static final Map<Class<?>, IConstTpl> constants = new HashMap<>();
    private static IJsonParser jsonParser = new SimpleJsonParser();
    private static SimpleXmlParser xmlParser = new SimpleXmlParser();
    private static String dataDir;
    private static String dataExt;

    public static void init(String dir) {
        dataDir = dir;
        dataExt = ".xml";
        loadAll();
    }

    public static void initJson(String dir) {
        initJson(dir, new SimpleJsonParser());
    }

    public static void initJson(String dir, IJsonParser parser) {
        dataDir = dir;
        dataExt = ".json";
        jsonParser = parser;
        loadAll();
    }

    private static void loadAll() {
        try {
            Files.walk(Paths.get(dataDir))
                .filter(p -> p.toString().endsWith(dataExt))
                .forEach(p -> {
                    TblMeta meta = parseFile(p);
                    if (meta != null) apply(meta);
                });
        } catch (Exception e) { throw new RuntimeException("Failed to scan " + dataDir, e); }
    }

    public static TblMeta parseFile(Path path) {
        try {
            Path rel = Paths.get(dataDir).relativize(path);
            String full = rel.toString().replace('\\', '/');
            String key = full.replaceFirst("\\.(json|xml)$", "");
            Class<?> clazz = TplRegistry.get(key);
            if (clazz == null) return null;

            String content = new String(Files.readAllBytes(path), "UTF-8");
            SepConfig sep = parseSep(content, full);

            if (ITpl.class.isAssignableFrom(clazz)) {
                return TblMeta.table(key, clazz, sep, parseTableData(content, full));
            } else if (IConstTpl.class.isAssignableFrom(clazz)) {
                return TblMeta.constant(key, clazz, sep, parseConstData(content, full));
            }
            return null;
        } catch (Exception e) { throw new RuntimeException("Failed to parse " + path, e); }
    }

    public static void apply(TblMeta meta) {
        if (meta.isTable) {
            Map<Integer, ITpl> map = new HashMap<>();
            for (Map<String, String> item : meta.rows) {
                ITpl obj = (ITpl) fromMap(meta.clazz, item, meta.sep);
                map.put(obj.getId(), obj);
            }
            tables.put(meta.clazz, map);
        } else {
            IConstTpl obj = (IConstTpl) fromMap(meta.clazz, meta.constData, meta.sep);
            constants.put(meta.clazz, obj);
        }
    }

    public static void reload(String key) {
        Class<?> clazz = TplRegistry.get(key);
        if (clazz == null) throw new IllegalArgumentException("Unknown registry key: " + key);
        Path path = Paths.get(dataDir, key + dataExt);
        if (!Files.exists(path)) throw new IllegalArgumentException("Data file not found: " + path);
        TblMeta meta = parseFile(path);
        if (meta == null) throw new IllegalStateException("Failed to parse: " + path);
        apply(meta);
    }

    public static <T extends ITpl> T get(Class<T> clazz, int id) {
        Map<Integer, ? extends ITpl> map = tables.get(clazz);
        return map != null ? (T) map.get(id) : null;
    }

    public static <T extends ITpl> Map<Integer, T> getAll(Class<T> clazz) {
        return (Map<Integer, T>) tables.get(clazz);
    }

    public static <T extends IConstTpl> T getConst(Class<T> clazz) {
        return (T) constants.get(clazz);
    }

    private static SepConfig parseSep(String content, String path) {
        if (path.endsWith(".xml")) {
            return SepConfig.fromMap(xmlParser.parseRootAttrs(content));
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        return SepConfig.fromMap(toStringMap((Map<String, Object>) wrapper.getOrDefault("_sep", Collections.emptyMap())));
    }

    private static List<Map<String, String>> parseTableData(String content, String path) {
        if (path.endsWith(".xml")) {
            return xmlParser.parseArray(content);
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        List<Map<String, Object>> raw = (List<Map<String, Object>>) wrapper.get("data");
        return toStringMaps(raw);
    }

    private static Map<String, String> parseConstData(String content, String path) {
        if (path.endsWith(".xml")) {
            return xmlParser.parseObject(content);
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        Map<String, Object> raw = (Map<String, Object>) wrapper.getOrDefault("data", wrapper);
        return toStringMap(raw);
    }

    private static <T> T fromMap(Class<T> clazz, Map<String, String> map, SepConfig sep) {
        try {
            T obj = clazz.getDeclaredConstructor().newInstance();
            for (var field : clazz.getDeclaredFields()) {
                field.setAccessible(true);
                TblType ann = field.getAnnotation(TblType.class);
                if (ann == null) continue;
                String raw = map.get(field.getName());
                field.set(obj, TplUtil.parseField(raw, ann.value(), sep));
            }
            return obj;
        } catch (Exception e) { throw new RuntimeException(e); }
    }

    private static List<Map<String, String>> toStringMaps(List<Map<String, Object>> raw) {
        List<Map<String, String>> result = new ArrayList<>();
        for (Map<String, Object> m : raw) {
            result.add(toStringMap(m));
        }
        return result;
    }

    private static Map<String, String> toStringMap(Map<String, Object> raw) {
        Map<String, String> result = new LinkedHashMap<>();
        for (Map.Entry<String, Object> e : raw.entrySet()) {
            result.put(e.getKey(), e.getValue() == null ? "" : e.getValue().toString());
        }
        return result;
    }
}