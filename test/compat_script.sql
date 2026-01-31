PROMPT Compatibility script start
SET ECHO ON
SET TERMOUT ON
SET TRIMSPOOL ON
SET COLSEP '|'

DEFINE sample_value = 123
ACCEPT user_value PROMPT 'Enter user value:'
PROMPT Using substitution variables
SELECT '&sample_value' AS single_sub, '&&sample_value' AS double_sub FROM dual;

COLUMN name FORMAT A10
BREAK ON name
COMPUTE SUM OF id ON name
TTITLE 'Report Header'
BTITLE 'Report Footer'

CREATE TABLE oqt_test_table (
  id NUMBER,
  name VARCHAR2(10)
);

VARIABLE v_counter NUMBER
BEGIN
  :v_counter := 1;
  DBMS_OUTPUT.PUT_LINE('counter=' || :v_counter);
END;
/
PRINT v_counter

CREATE OR REPLACE PROCEDURE oqt_test_proc AS
BEGIN
  DBMS_OUTPUT.PUT_LINE('proc ran');
END;
/
EXEC oqt_test_proc;

SET SERVEROUTPUT ON
SPOOL compat_spool.log
DESC oqt_test_table

@test/compat_global.sql
@@compat_relative.sql

SPOOL OFF
PROMPT Compatibility script end
