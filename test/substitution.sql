PROMPT === [SUBSTITUTION] Start ===
PROMPT Single & should prompt each time.
SELECT '&v_text' AS text_one FROM dual;
SELECT '&v_text' AS text_two FROM dual;
PROMPT Double && should prompt once and reuse.
SELECT '&&v_text2' AS text_three FROM dual;
SELECT '&&v_text2' AS text_four FROM dual;
PROMPT === [SUBSTITUTION] End ===
