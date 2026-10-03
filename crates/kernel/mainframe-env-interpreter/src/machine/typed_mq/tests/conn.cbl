identification division.
program-id. CONNWARN.
data division.
working-storage section.
01 MANAGER pic x(48) value spaces.
01 HCONN pic s9(9) binary value 77.
01 CC pic s9(9) binary value 17.
01 REASON pic s9(9) binary value 19.
procedure division.
    call 'MQCONN' using by reference MANAGER HCONN CC REASON
    call 'MQCMIT' using by reference HCONN CC REASON
    call 'MQBACK' using by reference HCONN CC REASON
    call 'MQDISC' using by reference HCONN CC REASON
    stop run.
