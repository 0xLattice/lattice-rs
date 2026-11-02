import Lean.Data.Json
import LatticeProofs.Work

open Lean Lattice.Work

def natural (j : Json) (key : String) : Except String Nat := do
  let s ← (← j.getObjVal? key).getStr?
  let some n := s.toNat? | throw s!"{key}: not a natural"
  unless n < 2^256 do throw s!"{key}: outside UInt256"
  return n

def check (j : Json) : Except String Unit := do
  let t ← natural j "target"
  let h ← natural j "hash"
  let tw ← natural j "targetWork"
  let hw ← natural j "hashWork"
  let meets ← (← j.getObjVal? "meets").getBool?
  unless tw == workForTarget (2^256) t do throw "target work differs"
  unless hw == workForHash (2^256) h do throw "hash work differs"
  unless meets == decide (Meets t h) do throw "target boundary differs"

def main (args : List String) : IO UInt32 := do
  let [path] := args | IO.eprintln "usage: WorkCheck trace.json"; return 2
  let rows ← IO.ofExcept ((← IO.ofExcept (Json.parse (← IO.FS.readFile path))).getArr?)
  if rows.isEmpty then IO.eprintln "empty corpus"; return 2
  let mut bad := 0
  for i in [:rows.size] do
    match check rows[i]! with
    | .ok () => pure ()
    | .error e =>
      bad := bad + 1
      if bad ≤ 10 then IO.eprintln s!"case {i}: {e}"
  IO.println s!"{rows.size - bad}/{rows.size} work observations conform"
  return if bad == 0 then 0 else 1
