package {{PACKAGE}};

import java.io.*;
import java.util.*;

/**
 * Classpath 数据源实现
 * 从 JAR 资源或类路径加载配置文件（只读）
 * 适用于生产环境将配置打包到 JAR 内的场景
 */
public class ClasspathTblDataSource implements TblDataSource {
    private final String resourcePath;   // 资源路径，如 "/data/config" 或 "data/config"
    private final String format;
    private final ClassLoader classLoader;

    /**
     * 创建 Classpath 数据源（使用当前线程的 ClassLoader）
     * @param resourcePath 资源路径（如 "/data/config" 或 "data/config"，自动添加前导斜杠）
     * @param format 文件格式 ".xml" 或 ".json"
     */
    public ClasspathTblDataSource(String resourcePath, String format) {
        this(resourcePath, format, Thread.currentThread().getContextClassLoader());
    }

    /**
     * 创建 Classpath 数据源（使用指定 ClassLoader）
     * @param resourcePath 资源路径（如 "/data/config" 或 "data/config"，自动添加前导斜杠）
     * @param format 文件格式 ".xml" 或 ".json"
     * @param classLoader 类加载器
     */
    public ClasspathTblDataSource(String resourcePath, String format, ClassLoader classLoader) {
        this.resourcePath = resourcePath.startsWith("/") ? resourcePath : "/" + resourcePath;
        this.format = format;
        this.classLoader = classLoader;
    }

    @Override
    public String loadRawData(String key) {
        String path = resourcePath + "/" + key + format;
        try (InputStream is = classLoader.getResourceAsStream(path.substring(1))) {
            if (is == null) return null;
            return new String(readAllBytes(is), "UTF-8");
        } catch (IOException e) {
            throw new RuntimeException("Failed to read " + key, e);
        }
    }

    @Override
    public Set<String> listAvailableKeys() {
        // Classpath 无法遍历资源目录，返回空集合
        // 使用 init(source, packageName) 时通过注解扫描提供键列表
        return Collections.emptySet();
    }

    @Override
    public String getDataFormat() {
        return format;
    }

    @Override
    public boolean supportsWrite() {
        return false;  // Classpath 资源只读
    }

    @Override
    public void validateRequiredData(Set<String> requiredKeys) {
        // 覆写校验逻辑：逐个尝试加载而非预扫描
        Set<String> missing = new LinkedHashSet<>();
        for (String key : requiredKeys) {
            if (loadRawData(key) == null) {
                missing.add(key);
            }
        }
        if (!missing.isEmpty()) {
            throw new IllegalStateException("Missing required config files: " + missing);
        }
    }

    private byte[] readAllBytes(InputStream is) throws IOException {
        ByteArrayOutputStream buffer = new ByteArrayOutputStream();
        byte[] data = new byte[8192];
        int nRead;
        while ((nRead = is.read(data, 0, data.length)) != -1) {
            buffer.write(data, 0, nRead);
        }
        return buffer.toByteArray();
    }
}
