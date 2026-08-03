package {{PACKAGE}};

import java.util.*;

/**
 * XML 解析器接口
 */
public interface IXmlParser {
    /**
     * 解析 XML 为单个对象（Map）
     */
    Map<String, String> parseObject(String xml);

    /**
     * 解析 XML 为对象数组（List of Map）
     */
    List<Map<String, String>> parseArray(String xml);

    /**
     * 解析 XML 根节点属性
     */
    Map<String, String> parseRootAttrs(String xml);
}
