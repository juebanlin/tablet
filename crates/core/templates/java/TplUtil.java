package {{PACKAGE}};

import java.util.*;

public class TplUtil {

    public static Object defaultValue(Class<?> type) {
        if (type == int.class) return 0;
        if (type == long.class) return 0L;
        if (type == float.class) return 0f;
        if (type == double.class) return 0.0;
        if (type == boolean.class) return false;
        if (type == String.class) return "";
        if (type == List.class) return new ArrayList<>();
        if (type == Set.class) return new HashSet<>();
        if (type == Map.class) return new HashMap<>();
        return null;
    }

    public static Object parseField(String raw, String tblType, SepConfig sep) {
        Paradigm p = Paradigm.of(tblType);
        if (raw == null || raw.isEmpty()) return p.defaultValue();
        return p.parse(raw, tblType, sep);
    }

    // ========== 解析工具方法 ==========

    /**
     * 解析分隔符配置
     * @param content 文件内容
     * @param path 文件路径（用于判断格式）
     * @param xmlParser XML 解析器
     * @param jsonParser JSON 解析器
     */
    public static SepConfig parseSep(String content, String path, IXmlParser xmlParser, IJsonParser jsonParser) {
        if (path.endsWith(".xml")) {
            return SepConfig.fromMap(xmlParser.parseRootAttrs(content));
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        return SepConfig.fromMap(toStringMap((Map<String, Object>) wrapper.getOrDefault("_sep", Collections.emptyMap())));
    }

    /**
     * 解析表数据（多行）
     */
    public static List<Map<String, String>> parseTableData(String content, String path, IXmlParser xmlParser, IJsonParser jsonParser) {
        if (path.endsWith(".xml")) {
            return xmlParser.parseArray(content);
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        List<Map<String, Object>> raw = (List<Map<String, Object>>) wrapper.get("data");
        return toStringMaps(raw);
    }

    /**
     * 解析常量数据（单行）
     */
    public static Map<String, String> parseConstData(String content, String path, IXmlParser xmlParser, IJsonParser jsonParser) {
        if (path.endsWith(".xml")) {
            return xmlParser.parseObject(content);
        }
        Map<String, Object> wrapper = jsonParser.parseObject(content);
        Map<String, Object> raw = (Map<String, Object>) wrapper.getOrDefault("data", wrapper);
        return toStringMap(raw);
    }

    /**
     * 从原始内容解析为 TblMeta
     * @param key 配置表名
     * @param clazz 配置类
     * @param content 文件内容
     * @param dataExt 文件扩展名（.xml 或 .json）
     * @param xmlParser XML 解析器
     * @param jsonParser JSON 解析器
     */
    public static TblMeta parseFromRaw(String key, Class<?> clazz, String content, String dataExt,
                                       IXmlParser xmlParser, IJsonParser jsonParser) {
        try {
            String full = key + dataExt;
            SepConfig sep = parseSep(content, full, xmlParser, jsonParser);

            if (ITpl.class.isAssignableFrom(clazz)) {
                return TblMeta.table(key, clazz, sep, parseTableData(content, full, xmlParser, jsonParser));
            } else if (IConstTpl.class.isAssignableFrom(clazz)) {
                return TblMeta.constant(key, clazz, sep, parseConstData(content, full, xmlParser, jsonParser));
            }
            return null;
        } catch (Exception e) {
            throw new RuntimeException("Failed to parse " + key, e);
        }
    }

    /**
     * 从 Map 反射创建配置对象（核心方法）
     * @param tableName 配置表名
     * @param clazz 配置类
     * @param map 字段数据
     * @param sep 分隔符配置
     * @param mismatchHandler 字段不匹配处理器（可为 null）
     */
    public static <T> T fromMap(String tableName, Class<T> clazz, Map<String, String> map, SepConfig sep,
                                TblFieldMismatchHandler mismatchHandler) {
        try {
            T obj = clazz.getDeclaredConstructor().newInstance();

            // 收集配置类字段名
            Set<String> classFields = new HashSet<>();

            for (java.lang.reflect.Field field : clazz.getDeclaredFields()) {
                field.setAccessible(true);
                TblFieldType ann = field.getAnnotation(TblFieldType.class);
                if (ann == null) continue;

                // 数据文件的 key 是 .tbl 源字段名（snake_case），字段名是 camelCase，
                // 两者不一致时注解里的 name 才是真 key；一致时注解留空、回退到字段名
                String key = ann.name().isEmpty() ? field.getName() : ann.name();

                classFields.add(key);
                String raw = map.get(key);

                // 验证：配置类字段在数据源中缺失
                if (raw == null && mismatchHandler != null) {
                    mismatchHandler.onClassFieldMissing(tableName, clazz.getSimpleName(), key, ann.value());
                }

                if (field.getType().isEnum() && ann.value().startsWith("@")) {
                    Object val = (raw == null || raw.isEmpty()) ? null
                        : field.getType().getMethod("fromId", int.class).invoke(null, Integer.parseInt(raw));
                    field.set(obj, val);
                } else if (ann.value().trim().startsWith("@")) {
                    // 顶层 table ref：字段类型是 int，值永远是 int id
                    field.set(obj, (raw == null || raw.isEmpty()) ? 0 : Integer.parseInt(raw));
                } else {
                    field.set(obj, parseField(raw, ann.value(), sep));
                }
            }

            // 验证：数据源字段在配置类中不存在
            if (mismatchHandler != null) {
                for (Map.Entry<String, String> entry : map.entrySet()) {
                    String fieldName = entry.getKey();
                    if (!classFields.contains(fieldName)) {
                        mismatchHandler.onSourceExtraField(tableName, clazz.getSimpleName(), fieldName, entry.getValue());
                    }
                }
            }

            return obj;
        } catch (Exception e) {
            throw new RuntimeException(e);
        }
    }

    // ========== 类型转换工具 ==========

    public static List<Map<String, String>> toStringMaps(List<Map<String, Object>> raw) {
        List<Map<String, String>> result = new ArrayList<>();
        for (Map<String, Object> m : raw) {
            result.add(toStringMap(m));
        }
        return result;
    }

    public static Map<String, String> toStringMap(Map<String, Object> raw) {
        Map<String, String> result = new LinkedHashMap<>();
        for (Map.Entry<String, Object> e : raw.entrySet()) {
            result.put(e.getKey(), e.getValue() == null ? "" : e.getValue().toString());
        }
        return result;
    }
}
