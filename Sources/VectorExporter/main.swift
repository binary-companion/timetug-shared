import Foundation

// Usage: VectorExporter <output-directory>
// Writes every vector file under <output-directory> (see vectors/README.md). Deterministic: two runs write identical files.
guard CommandLine.arguments.count == 2 else {
    FileHandle.standardError.write(Data("usage: VectorExporter <output-directory>\n".utf8))
    exit(2)
}
let outputDirectory = URL(fileURLWithPath: CommandLine.arguments[1], isDirectory: true)

do {
    try VectorFile.write(area: "text/fold", cases: TextAreas.foldCases(), minimum: 2000, to: outputDirectory)
    try VectorFile.write(area: "text/case-compare", cases: TextAreas.caseCompareCases(), minimum: 300, to: outputDirectory)
    try VectorFile.write(area: "text/dedup-key", cases: TextAreas.dedupKeyCases(), minimum: 100, to: outputDirectory)
    try VectorFile.write(area: "dedup/pair-decisions", cases: DedupAreas.pairCases(), minimum: 600, to: outputDirectory)
    try VectorFile.write(area: "dedup/resolutions", cases: DedupAreas.resolutionCases(), minimum: 300, to: outputDirectory)
    try VectorFile.write(area: "takeover/scheduler", cases: TakeoverAreas.schedulerCases(), minimum: 400, to: outputDirectory)
    try VectorFile.write(area: "takeover/ledger", cases: TakeoverAreas.ledgerCases(), minimum: 150, to: outputDirectory)
    try VectorFile.write(area: "agenda/day-agenda", cases: AgendaAreas.agendaCases(), minimum: 300, to: outputDirectory)
    try VectorFile.write(area: "widget/snapshot", cases: AgendaAreas.widgetCases(), minimum: 300, to: outputDirectory)
    try VectorFile.write(area: "wire/dates", cases: WireAreas.cases(), minimum: 400, to: outputDirectory)
    try VectorFile.write(area: "inference/prompts", cases: InferenceAreas.cases(), minimum: 150, to: outputDirectory)
} catch {
    FileHandle.standardError.write(Data("VectorExporter failed: \(error)\n".utf8))
    exit(1)
}
