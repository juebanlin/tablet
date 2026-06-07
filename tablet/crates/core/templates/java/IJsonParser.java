package {{PACKAGE}};

import java.util.Map;

public interface IJsonParser {
    Map<String, Object> parseObject(String json);
}
