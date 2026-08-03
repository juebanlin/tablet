package {{PACKAGE}};

import java.io.*;
import java.nio.file.*;
import java.util.*;

/**
 * 文件系统数据源实现
 * 从指定目录加载配置文件（XML 或 JSON）
 * 支持相对路径和绝对路径
 */
public class FileSystemTblDataSource implements TblDataSource {
    private final String baseDir;
    private final String format;  // ".xml" or ".json"

    public FileSystemTblDataSource(String baseDir, String format) {
        // 将相对路径转换为绝对路径，避免工作目录变化导致的问题
        this.baseDir = Paths.get(baseDir).toAbsolutePath().normalize().toString();
        this.format = format;
    }

    @Override
    public String loadRawData(String key) {
        try {
            Path path = Paths.get(baseDir, key + format);
            if (!Files.exists(path)) return null;
            return new String(Files.readAllBytes(path), "UTF-8");
        } catch (IOException e) {
            throw new RuntimeException("Failed to read " + key, e);
        }
    }

    @Override
    public Set<String> listAvailableKeys() {
        try {
            Set<String> keys = new LinkedHashSet<>();
            Files.walk(Paths.get(baseDir))
                .filter(p -> p.toString().endsWith(format))
                .forEach(p -> {
                    Path rel = Paths.get(baseDir).relativize(p);
                    String full = rel.toString().replace('\\', '/');
                    String key = full.replaceFirst("\\.(json|xml)$", "");
                    keys.add(key);
                });
            return keys;
        } catch (IOException e) {
            throw new RuntimeException("Failed to scan " + baseDir, e);
        }
    }

    @Override
    public String getDataFormat() {
        return format;
    }

    @Override
    public void updateConfig(String key, String content) {
        try {
            Path path = Paths.get(baseDir, key + format);
            Files.createDirectories(path.getParent());
            Files.write(path, content.getBytes("UTF-8"));
        } catch (IOException e) {
            throw new RuntimeException("Failed to write " + key, e);
        }
    }

    @Override
    public boolean supportsWrite() {
        return true;
    }

    @Override
    public Map<String, Boolean> updateConfigs(Map<String, String> configs) {
        Map<String, Boolean> results = new LinkedHashMap<>();
        for (Map.Entry<String, String> entry : configs.entrySet()) {
            try {
                updateConfig(entry.getKey(), entry.getValue());
                results.put(entry.getKey(), true);
            } catch (Exception e) {
                results.put(entry.getKey(), false);
            }
        }
        return results;
    }
}
