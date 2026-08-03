package {{PACKAGE}};

import java.io.File;
import java.util.HashMap;
import java.util.Map;

/**
 * 类扫描工具
 */
public class ClassUtil {

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
                        String key = annotation.group().isEmpty()
                            ? annotation.name()
                            : annotation.group() + "/" + annotation.name();
                        result.put(key, clazz);
                    }
                } catch (ClassNotFoundException | NoClassDefFoundError e) {
                    // 跳过无法加载的类
                }
            }
        }
    }
}
