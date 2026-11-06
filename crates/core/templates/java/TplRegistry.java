package {{PACKAGE}};

import java.io.File;
import java.util.*;

public class TplRegistry {
    private static final Map<String, Class<?>> map = new LinkedHashMap<>();

    static {
{{REGISTER_LIST}}
    }

    public static Map<String, Class<?>> getAll() {
        return Collections.unmodifiableMap(map);
    }

    public static Class<?> get(String key) {
        return map.get(key);
    }

    /**
     * 扫描指定包下所有标注了 @TblSource 的配置类
     * @param packageName 包名（如 "com.game.config.tpl"）
     * @return key → Class 映射
     */
    public static Map<String, Class<?>> scanTblSourceClasses(String packageName) {
        Map<String, Class<?>> result = new HashMap<>();
        try {
            String path = packageName.replace('.', '/');
            ClassLoader classLoader = Thread.currentThread().getContextClassLoader();
            java.net.URL resource = classLoader.getResource(path);

            if (resource == null) {
                return result;
            }

            File directory = new File(resource.toURI());
            if (!directory.exists()) {
                return result;
            }

            scanDirectory(directory, packageName, result);
        } catch (Exception e) {
            throw new RuntimeException("Failed to scan package: " + packageName, e);
        }
        return result;
    }

    /**
     * 递归扫描目录下的配置类
     */
    private static void scanDirectory(File directory, String packageName, Map<String, Class<?>> result) {
        File[] files = directory.listFiles();
        if (files == null) return;

        for (File file : files) {
            if (file.isDirectory()) {
                scanDirectory(file, packageName + "." + file.getName(), result);
            } else if (file.getName().endsWith(".class")) {
                String className = packageName + "." + file.getName().substring(0, file.getName().length() - 6);
                try {
                    Class<?> clazz = Class.forName(className);
                    TblSource annotation = clazz.getAnnotation(TblSource.class);
                    if (annotation != null) {
                        // enum 是编译期类型（XxxEnum.fromId），没有独立数据文件；
                        // 注册进来会让 TplHolder 去要一份不存在的 E.json。
                        if ("enum".equalsIgnoreCase(annotation.mode())) continue;
                        // 数据文件按节点名扁平存放（gen/server/data/json/<Name>.json）：
                        // 节点名在项目内唯一，key 直接用 name，不带 group 前缀。
                        result.put(annotation.name(), clazz);
                    }
                } catch (ClassNotFoundException | NoClassDefFoundError e) {
                    // 跳过无法加载的类
                }
            }
        }
    }
}
