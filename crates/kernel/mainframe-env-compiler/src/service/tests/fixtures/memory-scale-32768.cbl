identification division.
program-id. MEMSCALE.
data division.
working-storage section.
01 TABLE-AREA.
   05 CELL occurs 32768 times pic x(64) value spaces.
procedure division.
move 'FIRST' to CELL(1).
move 'LAST' to CELL(32768).
if CELL(1) = 'FIRST' and CELL(32768) = 'LAST' and CELL(16384) = spaces
    display 'SCALE-OK'
else
    display 'SCALE-FAIL'
end-if.
stop run.
