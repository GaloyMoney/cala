import React, { useState, useEffect } from 'react';
import CodeBlock from '@theme/CodeBlock';

export const GraphQLBody = ({ queryPath }) => {
  const [query, setQuery] = useState('');

  useEffect(() => {
    const loadQuery = async () => {
      try {
        const queryResponse = await fetch(queryPath);
        const queryText = await queryResponse.text();
        setQuery(queryText);
      } catch (error) {
        console.error('Error loading query:', error);
      }
    };

    loadQuery();
  }, [queryPath]);

  return (
    <CodeBlock className="language-graphql">
      {query}
    </CodeBlock>
  );
};

export const GraphQLVariables = ({ variablesPath }) => {
  const [variables, setVariables] = useState({});

  useEffect(() => {
    const loadVariables = async () => {
      try {
        const variablesResponse = await fetch(variablesPath);
        const variablesJson = await variablesResponse.json();
        setVariables(variablesJson);
      } catch (error) {
        console.error('Error loading variables:', error);
      }
    };

    loadVariables();
  }, [variablesPath]);

  return (
    <CodeBlock className="language-json">
      {JSON.stringify(variables, null, 2)}
    </CodeBlock>
  );
};

export const GraphQLResponse = ({ responsePath }) => {
  const [response, setResponse] = useState({});

  useEffect(() => {
    const loadResponse = async () => {
      try {
        const responseResponse = await fetch(responsePath);
        const responseJson = await responseResponse.json();
        setResponse(responseJson);
      } catch (error) {
        console.error('Error loading response:', error);
      }
    };

    loadResponse();
  }, [responsePath]);

  return (
    <CodeBlock className="language-json">
      {JSON.stringify(response, null, 2)}
    </CodeBlock>
  );
};
