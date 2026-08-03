package {{PACKAGE}};

import java.io.*;
import java.util.*;

/**
 * Classpath 数据源实现
 * 从 JAR 资源或类路径加载配置文件（只读）
 */
public class ClasspathTblDataSource implements TblDataSource {
    private final String basePath;   // 如 "/data/config"
    private final String format;
    private final ClassLoader classLoader;

    public ClasspathTblDataSource(String basePath, String format) {
        this(basePath, format, Thread.currentThread().getContextClassLoader());
    }

    public ClasspathTblDataSource(String basePath, String format, ClassLoader classLoader) {
        this.basePath = basePath.startsWith("/") ? basePath : "/" + basePath;
        this.format = format;
        this.classLoader = classLoader;
    }

    @Override
    public String loadRawData(String key) {
        String path = basePath + "/" + key + format;
        try (InputStream is = classLoader.getResourceAsStream(path.substring(1))) {
            if (is == null) return null;
            return new String(readAllBytes(is), "UTF-8");
        } catch (IOException e) {
            throw new RuntimeException("Failed to read " + key, e);
        }
    }

    @Override
    public Set<String> listAvailableKeys() {
        // Classpath 无法遍历，返回空集合
        // 使用 init(source, packageName) 时通过注解扫描提供键列表
        return Collections.emptySet();
    }

    @Override
    public String getDataFormat() {
        return format;
    }

    @Override
    public boolean supportsWrite() {
        return false;  // Classpath 只读
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
