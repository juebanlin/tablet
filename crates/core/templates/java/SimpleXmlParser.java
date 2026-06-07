package {{PACKAGE}};

import javax.xml.parsers.*;
import org.w3c.dom.*;
import java.io.*;
import java.util.*;

public class SimpleXmlParser {

    public Map<String, String> parseObject(String xml) {
        try {
            Document doc = parse(xml);
            Element root = doc.getDocumentElement();
            return elementToMap(root);
        } catch (Exception e) { throw new RuntimeException(e); }
    }

    public List<Map<String, String>> parseArray(String xml) {
        try {
            Document doc = parse(xml);
            Element root = doc.getDocumentElement();
            List<Map<String, String>> list = new ArrayList<>();
            NodeList items = root.getChildNodes();
            for (int i = 0; i < items.getLength(); i++) {
                Node node = items.item(i);
                if (node.getNodeType() == Node.ELEMENT_NODE) {
                    list.add(elementToMap((Element) node));
                }
            }
            return list;
        } catch (Exception e) { throw new RuntimeException(e); }
    }

    public Map<String, String> parseRootAttrs(String xml) {
        try {
            Document doc = parse(xml);
            Element root = doc.getDocumentElement();
            Map<String, String> attrs = new LinkedHashMap<>();
            NamedNodeMap attrMap = root.getAttributes();
            for (int i = 0; i < attrMap.getLength(); i++) {
                Node attr = attrMap.item(i);
                attrs.put(attr.getNodeName(), attr.getNodeValue());
            }
            return attrs;
        } catch (Exception e) { throw new RuntimeException(e); }
    }

    private Map<String, String> elementToMap(Element elem) {
        Map<String, String> map = new LinkedHashMap<>();
        NodeList children = elem.getChildNodes();
        for (int i = 0; i < children.getLength(); i++) {
            Node node = children.item(i);
            if (node.getNodeType() == Node.ELEMENT_NODE) {
                map.put(node.getNodeName(), node.getTextContent());
            }
        }
        return map;
    }

    private Document parse(String xml) throws Exception {
        DocumentBuilderFactory factory = DocumentBuilderFactory.newInstance();
        DocumentBuilder builder = factory.newDocumentBuilder();
        return builder.parse(new ByteArrayInputStream(xml.getBytes("UTF-8")));
    }
}
