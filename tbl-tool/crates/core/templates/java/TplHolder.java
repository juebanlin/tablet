package {{PACKAGE}};

import java.io.*;
import java.nio.file.*;
import java.util.*;

@SuppressWarnings("unchecked")
public class TplHolder {
    private static final Map<Class<?>, Map<Integer, ? extends ITpl>> tables = new HashMap<>();
    private static final Map<Class<?>, IConstTpl> constants = new HashMap<>();
    private static final Map<String, Class<?>> registry = new HashMap<>();
    private static IJsonParser jsonParser = new SimpleJsonParser();
    private static SimpleXmlParser xmlParser = new SimpleXmlParser();
    private static String dataDir;

    static {
{{REGISTER_LIST}}
    }

    public static void init(String dir) {
        dataDir = dir;
        loadAll(".xml");
    }

    public static void initJson(String dir) {
        initJson(dir, new SimpleJsonParser());
    }

    public static void initJson(String dir, IJsonParser parser) {
        dataDir = dir;
        jsonParser = parser;
        loadAll(".json");
    }

    private static void loadAll(String ext) {
        try {
            Files.walk(Paths.get(dataDir))
                .filter(p -> p.toString().endsWith(ext))
                .forEach(p -> {
                    Path rel = Paths.get(dataDir).relativize(p);
                    String full = rel.toString().replace('\\', '/');
                    String key = full.replaceFirst("\\.(json|xml)$", "");
                    Class<?> clazz = registry.get(key);
                    if (clazz == null) return;
                    if (ITpl.class.isAssignableFrom(clazz)) {
                        loadTable(full, (Class<? extends ITpl>) clazz);
                    } else if (IConstTpl.class.isAssignableFrom(clazz)) {
                        loadConst(full, (Class<? extends IConstTpl>) clazz);
                    }
                });
        } catch (Exception e) { throw new RuntimeException("Failed to scan " + dataDir, e); }
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

    private static <T extends ITpl> void loadTable(String path, Class<T> clazz) {
        try {
            String content = new String(Files.readAllBytes(Paths.get(dataDir, path)), "UTF-8");
            List<Map<String, String>> list;
            SepConfig sep;
            if (path.endsWith(".xml")) {
                list = xmlParser.parseArray(content);
                sep = SepConfig.fromMap(xmlParser.parseRootAttrs(content));
            } else {
                Map<String, Object> wrapper = jsonParser.parseObject(content);
                List<Map<String, Object>> raw = (List<Map<String, Object>>) wrapper.get("data");
                sep = SepConfig.fromMap(toStringMap((Map<String, Object>) wrapper.getOrDefault("_sep", Collections.emptyMap())));
                list = toStringMaps(raw);
            }
            Map<Integer, T> map = new HashMap<>();
            for (Map<String, String> item : list) {
                T obj = fromMap(clazz, item, sep);
                map.put(obj.getId(), obj);
            }
            tables.put(clazz, map);
        } catch (Exception e) { throw new RuntimeException("Failed to load " + path, e); }
    }

    private static <T extends IConstTpl> void loadConst(String path, Class<T> clazz) {
        try {
            String content = new String(Files.readAllBytes(Paths.get(dataDir, path)), "UTF-8");
            Map<String, String> data;
            SepConfig sep;
            if (path.endsWith(".xml")) {
                data = xmlParser.parseObject(content);
                sep = SepConfig.fromMap(xmlParser.parseRootAttrs(content));
            } else {
                Map<String, Object> wrapper = jsonParser.parseObject(content);
                Map<String, Object> raw = (Map<String, Object>) wrapper.getOrDefault("data", wrapper);
                sep = SepConfig.fromMap(toStringMap((Map<String, Object>) wrapper.getOrDefault("_sep", Collections.emptyMap())));
                data = toStringMap(raw);
            }
            T obj = fromMap(clazz, data, sep);
            constants.put(clazz, obj);
        } catch (Exception e) { throw new RuntimeException("Failed to load " + path, e); }
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