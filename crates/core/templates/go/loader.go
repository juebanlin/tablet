package {{PACKAGE}}

import (
	"encoding/json"
	"encoding/xml"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// 注册表：每个生成的 *_tpl.go 在 init() 中通过 register* 把自己加入 registry
var (
	tableRegistry    = map[string]tableLoader{}
	constantRegistry = map[string]constantLoader{}
)

type tableLoader func(rows []map[string]string, sep SepConfig)
type constantLoader func(row map[string]string, sep SepConfig)

func registerTable(key string, fn tableLoader) {
	tableRegistry[key] = fn
}

func registerConstant(key string, fn constantLoader) {
	constantRegistry[key] = fn
}

// === 入口 ===

// Init 加载 dataDir 下的 XML 数据文件（默认）
func Init(dataDir string) error {
	return loadAll(dataDir, ".xml")
}

// InitJSON 加载 dataDir 下的 JSON 数据文件
func InitJSON(dataDir string) error {
	return loadAll(dataDir, ".json")
}

func loadAll(dataDir, ext string) error {
	return filepath.Walk(dataDir, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.IsDir() || !strings.HasSuffix(path, ext) {
			return nil
		}
		rel, err := filepath.Rel(dataDir, path)
		if err != nil {
			return err
		}
		key := strings.TrimSuffix(filepath.ToSlash(rel), ext)
		return loadFile(path, ext, key)
	})
}

func loadFile(path, ext, key string) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}

	if tblFn, ok := tableRegistry[key]; ok {
		sep, rows, err := parseTableFile(data, ext)
		if err != nil {
			return fmt.Errorf("parse table %s: %w", path, err)
		}
		tblFn(rows, sep)
		return nil
	}

	if constFn, ok := constantRegistry[key]; ok {
		sep, row, err := parseConstantFile(data, ext)
		if err != nil {
			return fmt.Errorf("parse constant %s: %w", path, err)
		}
		constFn(row, sep)
		return nil
	}

	// 不在 registry 中的文件忽略
	return nil
}

// === JSON 解析 ===

type jsonWrapper struct {
	Sep  map[string]json.RawMessage `json:"_sep"`
	Data json.RawMessage            `json:"data"`
}

func parseTableFile(data []byte, ext string) (SepConfig, []map[string]string, error) {
	if ext == ".json" {
		return parseTableJSON(data)
	}
	return parseTableXML(data)
}

func parseConstantFile(data []byte, ext string) (SepConfig, map[string]string, error) {
	if ext == ".json" {
		return parseConstantJSON(data)
	}
	return parseConstantXML(data)
}

func parseTableJSON(data []byte) (SepConfig, []map[string]string, error) {
	var wrap jsonWrapper
	if err := json.Unmarshal(data, &wrap); err != nil {
		return SepConfig{}, nil, err
	}
	sep := sepFromJSONMap(jsonMapToString(wrap.Sep))

	var rawRows []map[string]json.RawMessage
	if err := json.Unmarshal(wrap.Data, &rawRows); err != nil {
		return SepConfig{}, nil, err
	}
	rows := make([]map[string]string, 0, len(rawRows))
	for _, r := range rawRows {
		rows = append(rows, jsonMapToString(r))
	}
	return sep, rows, nil
}

func parseConstantJSON(data []byte) (SepConfig, map[string]string, error) {
	var wrap jsonWrapper
	if err := json.Unmarshal(data, &wrap); err != nil {
		return SepConfig{}, nil, err
	}
	sep := sepFromJSONMap(jsonMapToString(wrap.Sep))

	var rawRow map[string]json.RawMessage
	if err := json.Unmarshal(wrap.Data, &rawRow); err != nil {
		return SepConfig{}, nil, err
	}
	return sep, jsonMapToString(rawRow), nil
}

// JSON 字段值统一转字符串：null → ""，数字/bool 转字面量，字符串去引号
func jsonMapToString(raw map[string]json.RawMessage) map[string]string {
	out := map[string]string{}
	for k, v := range raw {
		out[k] = jsonRawToString(v)
	}
	return out
}

func jsonRawToString(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	s := string(raw)
	if s == "null" {
		return ""
	}
	// 字符串：去掉首尾引号 + 反转义
	if len(s) >= 2 && s[0] == '"' && s[len(s)-1] == '"' {
		var unq string
		if err := json.Unmarshal(raw, &unq); err == nil {
			return unq
		}
	}
	return s
}

// === XML 解析 ===

type xmlElement struct {
	XMLName  xml.Name
	Attrs    []xml.Attr   `xml:"-"`
	Children []xmlElement `xml:",any"`
	Text     string       `xml:",chardata"`
}

// 通过 UnmarshalXML 收集 attrs（因为 xml.Attr 标签不能用 ",attr"）
func (e *xmlElement) UnmarshalXML(d *xml.Decoder, start xml.StartElement) error {
	e.XMLName = start.Name
	e.Attrs = start.Attr
	for {
		tok, err := d.Token()
		if err == io.EOF {
			break
		}
		if err != nil {
			return err
		}
		switch t := tok.(type) {
		case xml.StartElement:
			var child xmlElement
			if err := child.UnmarshalXML(d, t); err != nil {
				return err
			}
			e.Children = append(e.Children, child)
		case xml.CharData:
			e.Text += string(t)
		case xml.EndElement:
			return nil
		}
	}
	return nil
}

func parseTableXML(data []byte) (SepConfig, []map[string]string, error) {
	var root xmlElement
	if err := xml.Unmarshal(data, &root); err != nil {
		return SepConfig{}, nil, err
	}
	sep := sepFromXMLAttrs(attrsToMap(root.Attrs))

	rows := make([]map[string]string, 0, len(root.Children))
	for _, item := range root.Children {
		rows = append(rows, elementToMap(&item))
	}
	return sep, rows, nil
}

func parseConstantXML(data []byte) (SepConfig, map[string]string, error) {
	var root xmlElement
	if err := xml.Unmarshal(data, &root); err != nil {
		return SepConfig{}, nil, err
	}
	sep := sepFromXMLAttrs(attrsToMap(root.Attrs))
	return sep, elementToMap(&root), nil
}

func attrsToMap(attrs []xml.Attr) map[string]string {
	m := map[string]string{}
	for _, a := range attrs {
		m[a.Name.Local] = a.Value
	}
	return m
}

func elementToMap(elem *xmlElement) map[string]string {
	m := map[string]string{}
	for _, c := range elem.Children {
		m[c.XMLName.Local] = strings.TrimSpace(c.Text)
	}
	return m
}
