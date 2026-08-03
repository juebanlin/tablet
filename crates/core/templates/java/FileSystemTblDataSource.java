package {{PACKAGE}};

import java.io.*;
import java.nio.file.*;
import java.util.*;

/**
 * 文件系统数据源实现
 * 从指定目录加载配置文件（XML 或 JSON）
 * 支持相对路径（相对于当前工作目录）和绝对路径
 */
public class FileSystemTblDataSource implements TblDataSource {
    private final String path;
    private final String format;  // ".xml" or ".json"

    /**
     * 创建文件系统数据源
     * @param path 配置文件目录路径（支持相对路径和绝对路径，自动规范化）
     * @param format 文件格式 ".xml" 或 ".json"
     */
    public FileSystemTblDataSource(String path, String format) {
        // 将相对路径转换为绝对路径并规范化，避免工作目录变化导致的问题
        this.path = Paths.get(path).toAbsolutePath().normalize().toString();
        this.format = format;
    }

    @Override
    public String loadRawData(String key) {
        try {
            Path filePath = Paths.get(path, key + format);
            if (!Files.exists(filePath)) return null;
            return new String(Files.readAllBytes(filePath), "UTF-8");
        } catch (IOException e) {
            throw new RuntimeException("Failed to read " + key, e);
        }
    }

    @Override
    public Set<String> listAvailableKeys() {
        try {
            Set<String> keys = new LinkedHashSet<>();
            Files.walk(Paths.get(path))
                .filter(p -> p.toString().endsWith(format))
                .forEach(p -> {
                    Path rel = Paths.get(path).relativize(p);
                    String full = rel.toString().replace('\\', '/');
                    String key = full.replaceFirst("\\.(json|xml)$", "");
                    keys.add(key);
                });
            return keys;
        } catch (IOException e) {
            throw new RuntimeException("Failed to scan " + path, e);
        }
    }

    @Override
    public String getDataFormat() {
        return format;
    }

    @Override
    public void updateConfig(String key, String content) {
        try {
            Path filePath = Paths.get(path, key + format);
            Files.createDirectories(filePath.getParent());
            Files.write(filePath, content.getBytes("UTF-8"));
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
