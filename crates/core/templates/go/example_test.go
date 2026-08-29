package {{PACKAGE}}

import (
	"fmt"
	"os"
)

// ExampleInit 展示配置加载与查询的典型用法。
// - 运行：`go test ./{{PACKAGE}} -run ExampleInit`（无数据目录时自动跳过）。
// - 具体表名/字段见各 *_tpl.go 生成文件。
func ExampleInit() {
	if _, err := os.Stat("data/json"); err != nil {
		fmt.Println("（未找到 data/json，示例跳过）")
		return
	}
	InitJSON("data/json")

	// 单查：按主键取一行（id 表必有 Id 字段）
	if h := Get{{FIRST_TABLE}}(1); h != nil {
		fmt.Printf("id=%d %+v\n", h.Id, *h)
	}
	// 全表遍历
	fmt.Printf("{{FIRST_TABLE}} 行数=%d\n", len(GetAll{{FIRST_TABLE}}()))
}
