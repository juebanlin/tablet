package {{PACKAGE}};

import java.util.List;
import java.util.Map;

public interface IJsonParser {
    Map<String, Object> parseObject(String json);
    List<Map<String, Object>> parseArray(String json);
}
