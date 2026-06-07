package {{PACKAGE}}

import "strings"

// === Tuple 类型 ===
//
// 用泛型结构体表达 Tuple<A,B,...>，字段 F0/F1/F2/F3 对应位置参数。

type Tuple2[A, B any] struct {
	F0 A
	F1 B
}

type Tuple3[A, B, C any] struct {
	F0 A
	F1 B
	F2 C
}

type Tuple4[A, B, C, D any] struct {
	F0 A
	F1 B
	F2 C
	F3 D
}

// === 通用 Tuple 解析 ===

func ParseTuple2[A, B any](raw, sep string, fa func(string) A, fb func(string) B) Tuple2[A, B] {
	parts := strings.SplitN(raw, sep, 2)
	var t Tuple2[A, B]
	if len(parts) > 0 {
		t.F0 = fa(strings.TrimSpace(parts[0]))
	}
	if len(parts) > 1 {
		t.F1 = fb(strings.TrimSpace(parts[1]))
	}
	return t
}

func ParseTuple3[A, B, C any](raw, sep string, fa func(string) A, fb func(string) B, fc func(string) C) Tuple3[A, B, C] {
	parts := strings.SplitN(raw, sep, 3)
	var t Tuple3[A, B, C]
	if len(parts) > 0 {
		t.F0 = fa(strings.TrimSpace(parts[0]))
	}
	if len(parts) > 1 {
		t.F1 = fb(strings.TrimSpace(parts[1]))
	}
	if len(parts) > 2 {
		t.F2 = fc(strings.TrimSpace(parts[2]))
	}
	return t
}

func ParseTuple4[A, B, C, D any](raw, sep string, fa func(string) A, fb func(string) B, fc func(string) C, fd func(string) D) Tuple4[A, B, C, D] {
	parts := strings.SplitN(raw, sep, 4)
	var t Tuple4[A, B, C, D]
	if len(parts) > 0 {
		t.F0 = fa(strings.TrimSpace(parts[0]))
	}
	if len(parts) > 1 {
		t.F1 = fb(strings.TrimSpace(parts[1]))
	}
	if len(parts) > 2 {
		t.F2 = fc(strings.TrimSpace(parts[2]))
	}
	if len(parts) > 3 {
		t.F3 = fd(strings.TrimSpace(parts[3]))
	}
	return t
}

// === List<Tuple_> / Map / Map<_, Tuple_> / Map<_, List<_>> ===

func ParseListItems[T any](raw, listSep string, parseItem func(string) T) []T {
	parts := splitTrim(raw, listSep)
	out := make([]T, 0, len(parts))
	for _, p := range parts {
		out = append(out, parseItem(p))
	}
	return out
}

func ParseMap[K comparable, V any](raw, kvSep, entrySep string, parseKey func(string) K, parseVal func(string) V) map[K]V {
	m := map[K]V{}
	for _, entry := range splitTrim(raw, entrySep) {
		k, v, ok := splitKV(entry, kvSep)
		if !ok {
			continue
		}
		m[parseKey(k)] = parseVal(v)
	}
	return m
}
