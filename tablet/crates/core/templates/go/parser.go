package {{PACKAGE}}

import (
	"strconv"
	"strings"
)

// === 基础类型解析 ===

func parseInt32(raw string) int32 {
	v, _ := strconv.ParseInt(strings.TrimSpace(raw), 10, 32)
	return int32(v)
}

func parseInt64(raw string) int64 {
	v, _ := strconv.ParseInt(strings.TrimSpace(raw), 10, 64)
	return v
}

func parseFloat32(raw string) float32 {
	v, _ := strconv.ParseFloat(strings.TrimSpace(raw), 32)
	return float32(v)
}

func parseFloat64(raw string) float64 {
	v, _ := strconv.ParseFloat(strings.TrimSpace(raw), 64)
	return v
}

func parseBool(raw string) bool {
	r := strings.TrimSpace(raw)
	return r == "true" || r == "1"
}

// === List<T> ===

func parseListInt32(raw, sep string) []int32 {
	if raw == "" {
		return []int32{}
	}
	parts := strings.Split(raw, sep)
	out := make([]int32, 0, len(parts))
	for _, p := range parts {
		t := strings.TrimSpace(p)
		if t == "" {
			continue
		}
		out = append(out, parseInt32(t))
	}
	return out
}

func parseListInt64(raw, sep string) []int64 {
	if raw == "" {
		return []int64{}
	}
	parts := strings.Split(raw, sep)
	out := make([]int64, 0, len(parts))
	for _, p := range parts {
		t := strings.TrimSpace(p)
		if t == "" {
			continue
		}
		out = append(out, parseInt64(t))
	}
	return out
}

func parseListFloat32(raw, sep string) []float32 {
	if raw == "" {
		return []float32{}
	}
	parts := strings.Split(raw, sep)
	out := make([]float32, 0, len(parts))
	for _, p := range parts {
		t := strings.TrimSpace(p)
		if t == "" {
			continue
		}
		out = append(out, parseFloat32(t))
	}
	return out
}

func parseListFloat64(raw, sep string) []float64 {
	if raw == "" {
		return []float64{}
	}
	parts := strings.Split(raw, sep)
	out := make([]float64, 0, len(parts))
	for _, p := range parts {
		t := strings.TrimSpace(p)
		if t == "" {
			continue
		}
		out = append(out, parseFloat64(t))
	}
	return out
}

func parseListString(raw, sep string) []string {
	if raw == "" {
		return []string{}
	}
	parts := strings.Split(raw, sep)
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		t := strings.TrimSpace(p)
		if t == "" {
			continue
		}
		out = append(out, t)
	}
	return out
}

func parseListBool(raw, sep string) []bool {
	if raw == "" {
		return []bool{}
	}
	parts := strings.Split(raw, sep)
	out := make([]bool, 0, len(parts))
	for _, p := range parts {
		t := strings.TrimSpace(p)
		if t == "" {
			continue
		}
		out = append(out, parseBool(t))
	}
	return out
}

// === Set<T> 用 map[T]struct{} 表达 ===

func parseSetInt32(raw, sep string) map[int32]struct{} {
	out := map[int32]struct{}{}
	for _, v := range parseListInt32(raw, sep) {
		out[v] = struct{}{}
	}
	return out
}

func parseSetInt64(raw, sep string) map[int64]struct{} {
	out := map[int64]struct{}{}
	for _, v := range parseListInt64(raw, sep) {
		out[v] = struct{}{}
	}
	return out
}

func parseSetFloat32(raw, sep string) map[float32]struct{} {
	out := map[float32]struct{}{}
	for _, v := range parseListFloat32(raw, sep) {
		out[v] = struct{}{}
	}
	return out
}

func parseSetFloat64(raw, sep string) map[float64]struct{} {
	out := map[float64]struct{}{}
	for _, v := range parseListFloat64(raw, sep) {
		out[v] = struct{}{}
	}
	return out
}

func parseSetString(raw, sep string) map[string]struct{} {
	out := map[string]struct{}{}
	for _, v := range parseListString(raw, sep) {
		out[v] = struct{}{}
	}
	return out
}

// === 通用 split helper ===

// 按 sep 切分并 trim 空，跳过空段
func splitTrim(raw, sep string) []string {
	if raw == "" {
		return nil
	}
	parts := strings.Split(raw, sep)
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		t := strings.TrimSpace(p)
		if t == "" {
			continue
		}
		out = append(out, t)
	}
	return out
}

// 按 sep 切分一次（k:v）
func splitKV(raw, sep string) (string, string, bool) {
	idx := strings.Index(raw, sep)
	if idx < 0 {
		return "", "", false
	}
	return strings.TrimSpace(raw[:idx]), strings.TrimSpace(raw[idx+len(sep):]), true
}
