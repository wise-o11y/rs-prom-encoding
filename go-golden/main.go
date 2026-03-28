// Golden fixture generator for rs-prom-encoder compatibility testing.
//
// This program uses Prometheus's own chunkenc library to generate reference
// chunk bytes that the Rust implementation must match byte-for-byte.
package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"github.com/prometheus/prometheus/model/histogram"
	"github.com/prometheus/prometheus/model/value"
	"github.com/prometheus/prometheus/tsdb/chunkenc"
)

// FixtureMeta describes a test fixture for JSON serialization.
type FixtureMeta struct {
	Encoding string   `json:"encoding"`
	Samples  []Sample `json:"samples"`
}

// Sample represents a single time-series sample.
type Sample struct {
	Timestamp int64   `json:"t"`
	Value     float64 `json:"v,omitempty"`
}

// HistogramSample represents a histogram sample with full data.
type HistogramSample struct {
	Timestamp int64                `json:"t"`
	Histogram *histogram.Histogram `json:"h,omitempty"`
}

// FloatHistogramSample represents a float histogram sample.
type FloatHistogramSample struct {
	Timestamp      int64                     `json:"t"`
	FloatHistogram *histogram.FloatHistogram `json:"fh,omitempty"`
}

func main() {
	fixturesDir := "../tests/fixtures"
	if err := os.MkdirAll(fixturesDir, 0755); err != nil {
		panic(err)
	}

	// Generate all fixtures
	generateXORFixtures(fixturesDir)
	generateHistogramFixtures(fixturesDir)
	generateFloatHistogramFixtures(fixturesDir)

	fmt.Println("Golden fixtures generated successfully in", fixturesDir)
}

// writeFixture writes the binary chunk data and JSON metadata.
func writeFixture(dir, name string, chunk chunkenc.Chunk, meta interface{}) {
	// Write binary
	binPath := filepath.Join(dir, name+".bin")
	if err := os.WriteFile(binPath, chunk.Bytes(), 0644); err != nil {
		panic(fmt.Sprintf("failed to write %s: %v", binPath, err))
	}

	// Write JSON metadata
	jsonPath := filepath.Join(dir, name+".json")
	jsonData, err := json.MarshalIndent(meta, "", "  ")
	if err != nil {
		panic(fmt.Sprintf("failed to marshal JSON for %s: %v", name, err))
	}
	if err := os.WriteFile(jsonPath, jsonData, 0644); err != nil {
		panic(fmt.Sprintf("failed to write %s: %v", jsonPath, err))
	}

	fmt.Printf("Generated %s.bin (%d bytes) + %s.json\n", name, len(chunk.Bytes()), name)
}

// generateXORFixtures creates XOR chunk test fixtures.
func generateXORFixtures(dir string) {
	// xor_basic: 5 samples, constant 15s spacing, increasing values
	{
		chunk := chunkenc.NewXORChunk()
		app, _ := chunk.Appender()
		baseTime := int64(1000000)
		samples := []Sample{}
		for i := 0; i < 5; i++ {
			t := baseTime + int64(i)*15000
			v := 1.0 + float64(i)*0.5
			app.Append(0, t, v) // st=0 (not used by XOR)
			samples = append(samples, Sample{Timestamp: t, Value: v})
		}
		writeFixture(dir, "xor_basic", chunk, FixtureMeta{
			Encoding: "XOR",
			Samples:  samples,
		})
	}

	// xor_stale: 5 samples with stale NaN at sample 3
	{
		chunk := chunkenc.NewXORChunk()
		app, _ := chunk.Appender()
		baseTime := int64(1000000)
		samples := []Sample{}
		for i := 0; i < 5; i++ {
			t := baseTime + int64(i)*15000
			var v float64
			if i == 2 {
				v = float64(value.StaleNaN)
			} else {
				v = 1.0 + float64(i)*0.5
			}
			app.Append(0, t, v)
			samples = append(samples, Sample{Timestamp: t, Value: v})
		}
		writeFixture(dir, "xor_stale", chunk, FixtureMeta{
			Encoding: "XOR",
			Samples:  samples,
		})
	}

	// xor_edge_cases: various edge cases
	{
		chunk := chunkenc.NewXORChunk()
		app, _ := chunk.Appender()
		baseTime := int64(1000000)
		samples := []Sample{
			{Timestamp: baseTime, Value: 0.0},
			{Timestamp: baseTime + 15000, Value: -1.5},
			{Timestamp: baseTime + 15000 + 1000000, Value: 1e10}, // large dod
			{Timestamp: baseTime + 15000 + 1000000 + 1, Value: 1e-10},
			{Timestamp: baseTime + 15000 + 1000000 + 1 + 15000, Value: 0.0},
		}
		for _, s := range samples {
			app.Append(0, s.Timestamp, s.Value)
		}
		writeFixture(dir, "xor_edge_cases", chunk, FixtureMeta{
			Encoding: "XOR",
			Samples:  samples,
		})
	}

	// xor_many: 120 samples (typical chunk size)
	{
		chunk := chunkenc.NewXORChunk()
		app, _ := chunk.Appender()
		baseTime := int64(1700000000000)
		samples := make([]Sample, 120)
		for i := 0; i < 120; i++ {
			t := baseTime + int64(i)*15000
			v := 100.0 + float64(i)*0.1
			samples[i] = Sample{Timestamp: t, Value: v}
			app.Append(0, t, v)
		}
		writeFixture(dir, "xor_many", chunk, FixtureMeta{
			Encoding: "XOR",
			Samples:  samples,
		})
	}
}

// generateHistogramFixtures creates integer histogram chunk test fixtures.
func generateHistogramFixtures(dir string) {
	// histogram_basic: 3 samples with same schema
	{
		chunk := chunkenc.NewHistogramChunk()
		app, err := chunk.Appender()
		if err != nil {
			panic(err)
		}
		baseTime := int64(1000000)

		h1 := &histogram.Histogram{
			Count:         10,
			ZeroCount:     2,
			Sum:           18.4,
			Schema:        3,
			ZeroThreshold: 2.938735877055719e-39, // 2^-128
			PositiveSpans: []histogram.Span{
				{Offset: 0, Length: 2},
			},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{4, 4},
			NegativeBuckets: []int64{},
		}

		h2 := &histogram.Histogram{
			Count:           15,
			ZeroCount:       3,
			Sum:             28.0,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{6, 6},
			NegativeBuckets: []int64{},
		}

		h3 := &histogram.Histogram{
			Count:           22,
			ZeroCount:       5,
			Sum:             40.5,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{8, 9},
			NegativeBuckets: []int64{},
		}

		hists := []*histogram.Histogram{h1, h2, h3}
		for i, h := range hists {
			_, _, newApp, err := app.AppendHistogram(nil, 0, baseTime+int64(i)*15000, h, false)
			if err != nil {
				panic(err)
			}
			app = newApp
		}

		// Store histogram samples for JSON
		samples := []HistogramSample{
			{Timestamp: baseTime, Histogram: h1},
			{Timestamp: baseTime + 15000, Histogram: h2},
			{Timestamp: baseTime + 30000, Histogram: h3},
		}
		writeFixture(dir, "histogram_basic", chunk, map[string]interface{}{
			"encoding": "Histogram",
			"samples":  samples,
		})
	}

	// histogram_stale: 4 samples with stale NaN at sample 3
	{
		chunk := chunkenc.NewHistogramChunk()
		app, err := chunk.Appender()
		if err != nil {
			panic(err)
		}
		baseTime := int64(1000000)

		h1 := &histogram.Histogram{
			Count:           10,
			ZeroCount:       2,
			Sum:             18.4,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{4, 4},
			NegativeBuckets: []int64{},
		}

		h2 := &histogram.Histogram{
			Count:           15,
			ZeroCount:       3,
			Sum:             28.0,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{6, 6},
			NegativeBuckets: []int64{},
		}

		// Stale sample - keep same bucket structure but mark as stale via sum
		h3 := &histogram.Histogram{
			Count:           15, // same count to force delta=0
			ZeroCount:       3,
			Sum:             float64(value.StaleNaN),
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{6, 6}, // same structure as h2
			NegativeBuckets: []int64{},
		}

		h4 := &histogram.Histogram{
			Count:           22,
			ZeroCount:       5,
			Sum:             40.5,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{8, 9},
			NegativeBuckets: []int64{},
		}

		hists := []*histogram.Histogram{h1, h2, h3, h4}
		for i, h := range hists {
			_, _, newApp, err := app.AppendHistogram(nil, 0, baseTime+int64(i)*15000, h, false)
			if err != nil {
				panic(err)
			}
			app = newApp
		}

		samples := []HistogramSample{
			{Timestamp: baseTime, Histogram: h1},
			{Timestamp: baseTime + 15000, Histogram: h2},
			{Timestamp: baseTime + 30000, Histogram: h3},
			{Timestamp: baseTime + 45000, Histogram: h4},
		}
		writeFixture(dir, "histogram_stale", chunk, map[string]interface{}{
			"encoding": "Histogram",
			"samples":  samples,
		})
	}

	// histogram_gauge: with GaugeType counter reset hint
	{
		chunk := chunkenc.NewHistogramChunk()
		app, err := chunk.Appender()
		if err != nil {
			panic(err)
		}
		baseTime := int64(1000000)

		// Set gauge type via the histogram's CounterResetHint field
		h1 := &histogram.Histogram{
			Count:            10,
			ZeroCount:        2,
			Sum:              18.4,
			Schema:           3,
			ZeroThreshold:    2.938735877055719e-39,
			PositiveSpans:    []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:    []histogram.Span{},
			PositiveBuckets:  []int64{4, 4},
			NegativeBuckets:  []int64{},
			CounterResetHint: histogram.GaugeType,
		}

		h2 := &histogram.Histogram{
			Count:            5, // Decreasing count (allowed for gauges)
			ZeroCount:        1,
			Sum:              8.0,
			Schema:           3,
			ZeroThreshold:    2.938735877055719e-39,
			PositiveSpans:    []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:    []histogram.Span{},
			PositiveBuckets:  []int64{2, 2},
			NegativeBuckets:  []int64{},
			CounterResetHint: histogram.GaugeType,
		}

		h3 := &histogram.Histogram{
			Count:            12,
			ZeroCount:        3,
			Sum:              22.5,
			Schema:           3,
			ZeroThreshold:    2.938735877055719e-39,
			PositiveSpans:    []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:    []histogram.Span{},
			PositiveBuckets:  []int64{5, 4},
			NegativeBuckets:  []int64{},
			CounterResetHint: histogram.GaugeType,
		}

		hists := []*histogram.Histogram{h1, h2, h3}
		for i, h := range hists {
			_, _, newApp, err := app.AppendHistogram(nil, 0, baseTime+int64(i)*15000, h, false)
			if err != nil {
				panic(err)
			}
			app = newApp
		}

		samples := []HistogramSample{
			{Timestamp: baseTime, Histogram: h1},
			{Timestamp: baseTime + 15000, Histogram: h2},
			{Timestamp: baseTime + 30000, Histogram: h3},
		}
		writeFixture(dir, "histogram_gauge", chunk, map[string]interface{}{
			"encoding": "Histogram",
			"samples":  samples,
		})
	}

	// histogram_custom_bounds: CustomBucketsSchema (-53) with custom bounds
	{
		chunk := chunkenc.NewHistogramChunk()
		app, err := chunk.Appender()
		if err != nil {
			panic(err)
		}
		baseTime := int64(1000000)

		customBounds := []float64{0.5, 1.0, 2.5, 5.0, 10.0}

		h1 := &histogram.Histogram{
			Count:           8,
			ZeroCount:       1,
			Sum:             15.3,
			Schema:          histogram.CustomBucketsSchema,
			ZeroThreshold:   0.25,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: uint32(len(customBounds))}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{2, 2, 1, 1, 1},
			NegativeBuckets: []int64{},
			CustomValues:    customBounds,
		}

		h2 := &histogram.Histogram{
			Count:           12,
			ZeroCount:       2,
			Sum:             24.6,
			Schema:          histogram.CustomBucketsSchema,
			ZeroThreshold:   0.25,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: uint32(len(customBounds))}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{3, 3, 2, 2, 1},
			NegativeBuckets: []int64{},
			CustomValues:    customBounds,
		}

		h3 := &histogram.Histogram{
			Count:           18,
			ZeroCount:       3,
			Sum:             38.9,
			Schema:          histogram.CustomBucketsSchema,
			ZeroThreshold:   0.25,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: uint32(len(customBounds))}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []int64{4, 5, 3, 3, 2},
			NegativeBuckets: []int64{},
			CustomValues:    customBounds,
		}

		hists := []*histogram.Histogram{h1, h2, h3}
		for i, h := range hists {
			_, _, newApp, err := app.AppendHistogram(nil, 0, baseTime+int64(i)*15000, h, false)
			if err != nil {
				panic(err)
			}
			app = newApp
		}

		samples := []HistogramSample{
			{Timestamp: baseTime, Histogram: h1},
			{Timestamp: baseTime + 15000, Histogram: h2},
			{Timestamp: baseTime + 30000, Histogram: h3},
		}
		writeFixture(dir, "histogram_custom_bounds", chunk, map[string]interface{}{
			"encoding": "Histogram",
			"samples":  samples,
		})
	}
}

// generateFloatHistogramFixtures creates float histogram chunk test fixtures.
func generateFloatHistogramFixtures(dir string) {
	// float_histogram_basic: 3 samples
	{
		chunk := chunkenc.NewFloatHistogramChunk()
		app, err := chunk.Appender()
		if err != nil {
			panic(err)
		}
		baseTime := int64(1000000)

		fh1 := &histogram.FloatHistogram{
			Count:           10.0,
			ZeroCount:       2.0,
			Sum:             18.4,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []float64{4.0, 4.0},
			NegativeBuckets: []float64{},
		}

		fh2 := &histogram.FloatHistogram{
			Count:           15.0,
			ZeroCount:       3.0,
			Sum:             28.0,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []float64{6.0, 6.0},
			NegativeBuckets: []float64{},
		}

		fh3 := &histogram.FloatHistogram{
			Count:           22.0,
			ZeroCount:       5.0,
			Sum:             40.5,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []float64{8.0, 9.0},
			NegativeBuckets: []float64{},
		}

		fhs := []*histogram.FloatHistogram{fh1, fh2, fh3}
		for i, fh := range fhs {
			_, _, newApp, err := app.AppendFloatHistogram(nil, 0, baseTime+int64(i)*15000, fh, false)
			if err != nil {
				panic(err)
			}
			app = newApp
		}

		samples := []FloatHistogramSample{
			{Timestamp: baseTime, FloatHistogram: fh1},
			{Timestamp: baseTime + 15000, FloatHistogram: fh2},
			{Timestamp: baseTime + 30000, FloatHistogram: fh3},
		}
		writeFixture(dir, "float_histogram_basic", chunk, map[string]interface{}{
			"encoding": "FloatHistogram",
			"samples":  samples,
		})
	}

	// float_histogram_stale: 4 samples with stale NaN at sample 3
	{
		chunk := chunkenc.NewFloatHistogramChunk()
		app, err := chunk.Appender()
		if err != nil {
			panic(err)
		}
		baseTime := int64(1000000)

		fh1 := &histogram.FloatHistogram{
			Count:           10.0,
			ZeroCount:       2.0,
			Sum:             18.4,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []float64{4.0, 4.0},
			NegativeBuckets: []float64{},
		}

		fh2 := &histogram.FloatHistogram{
			Count:           15.0,
			ZeroCount:       3.0,
			Sum:             28.0,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []float64{6.0, 6.0},
			NegativeBuckets: []float64{},
		}

		// Stale sample - keep same bucket structure but mark as stale via sum
		fh3 := &histogram.FloatHistogram{
			Count:           15.0,
			ZeroCount:       3.0,
			Sum:             float64(value.StaleNaN),
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []float64{6.0, 6.0}, // same structure as fh2
			NegativeBuckets: []float64{},
		}

		fh4 := &histogram.FloatHistogram{
			Count:           22.0,
			ZeroCount:       5.0,
			Sum:             40.5,
			Schema:          3,
			ZeroThreshold:   2.938735877055719e-39,
			PositiveSpans:   []histogram.Span{{Offset: 0, Length: 2}},
			NegativeSpans:   []histogram.Span{},
			PositiveBuckets: []float64{8.0, 9.0},
			NegativeBuckets: []float64{},
		}

		fhs := []*histogram.FloatHistogram{fh1, fh2, fh3, fh4}
		for i, fh := range fhs {
			_, _, newApp, err := app.AppendFloatHistogram(nil, 0, baseTime+int64(i)*15000, fh, false)
			if err != nil {
				panic(err)
			}
			app = newApp
		}

		samples := []FloatHistogramSample{
			{Timestamp: baseTime, FloatHistogram: fh1},
			{Timestamp: baseTime + 15000, FloatHistogram: fh2},
			{Timestamp: baseTime + 30000, FloatHistogram: fh3},
			{Timestamp: baseTime + 45000, FloatHistogram: fh4},
		}
		writeFixture(dir, "float_histogram_stale", chunk, map[string]interface{}{
			"encoding": "FloatHistogram",
			"samples":  samples,
		})
	}
}
